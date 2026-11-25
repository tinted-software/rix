//! P2P substituter for fetching and verifying Nix-like store closures via Iroh.

use crate::blob_store::{BlobStore, path_already_populated};
use crate::error::{Result, StoreError};
use crate::manifest::{PathInfo, SignedPathInfo};
use crate::p2p::dht::DhtClient;
use crate::p2p::node::P2pNode;
use crate::p2p::protocol::{Request, Response, rpc_call};
use iroh::{Endpoint, EndpointAddr, EndpointId, PublicKey};
use iroh_blobs::api::downloader::Downloader;
use iroh_blobs::{Hash, HashAndFormat};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Configuration for a P2P substitution session.
#[derive(Debug, Clone)]
pub struct SubstitutionConfig {
    pub peers: Vec<EndpointId>,
    pub known_addrs: Vec<EndpointAddr>,
    pub trusted_keys: Vec<PublicKey>,
    pub enable_dht: bool,
    pub use_relays: bool,
    pub allow_untrusted: bool,
    pub timeout: Duration,
}

impl Default for SubstitutionConfig {
    fn default() -> Self {
        Self {
            peers: Vec::new(),
            known_addrs: Vec::new(),
            trusted_keys: Vec::new(),
            enable_dht: true,
            use_relays: true,
            allow_untrusted: false,
            timeout: Duration::from_secs(15),
        }
    }
}

impl SubstitutionConfig {
    /// Returns `true` if there is at least one discovery target configured.
    pub fn has_discovery_sources(&self) -> bool {
        !self.peers.is_empty()
            || !self.known_addrs.is_empty()
            || (self.enable_dht && !self.trusted_keys.is_empty())
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    info: SignedPathInfo,
    providers: Vec<EndpointId>,
    trusted_via_dht: bool,
}

/// P2P substituter backed by an ephemeral non-seeding [`P2pNode`] and local [`BlobStore`].
pub struct Substituter {
    node: P2pNode,
    blob_store: BlobStore,
    downloader: Downloader,
    config: SubstitutionConfig,
}

impl Substituter {
    /// Create a non-seeding P2P substituter bound to `blob_store`.
    pub async fn new(blob_store: BlobStore, config: SubstitutionConfig) -> Result<Self> {
        let node = P2pNode::spawn_client(config.enable_dht, config.use_relays).await?;
        for addr in &config.known_addrs {
            node.add_peer_addr(addr.clone());
        }
        let downloader = blob_store.store().downloader(node.endpoint());
        Ok(Self {
            node,
            blob_store,
            downloader,
            config,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        self.node.endpoint()
    }

    pub async fn close(self) {
        self.node.close().await;
    }

    /// Attempt to substitute `store_key` (and its transitive closure) into `store_dir`.
    ///
    /// Returns `Ok(true)` if the path is now available and materialized on disk, or
    /// `Ok(false)` if no trusted provider was found.
    pub async fn substitute_path(&self, store_dir: &Path, store_key: &str) -> Result<bool> {
        let mut active = HashSet::new();
        self.substitute_recursive(store_dir, store_key, &mut active)
            .await
    }

    async fn substitute_recursive(
        &self,
        store_dir: &Path,
        store_key: &str,
        visiting: &mut HashSet<String>,
    ) -> Result<bool> {
        let dest_path = store_dir.join(store_key);
        if path_already_populated(&dest_path) {
            return Ok(true);
        }

        if self.blob_store.has_path(store_key).await? {
            self.blob_store
                .materialize_path(store_key, &dest_path)
                .await?;
            return Ok(true);
        }

        if !visiting.insert(store_key.to_string()) {
            return Err(StoreError::InvalidManifest(format!(
                "cyclic store reference involving '{store_key}'"
            )));
        }

        let candidates = self.discover_candidates(store_key).await;
        let selected = self.select_trusted_candidate(store_key, candidates)?;

        let Some(candidate) = selected else {
            visiting.remove(store_key);
            return Ok(false);
        };

        // Fetch the HashSeq + manifest + all file blobs with BLAKE3 verification.
        self.downloader
            .download(
                HashAndFormat::hash_seq(candidate.info.info.root_hash),
                candidate.providers.clone(),
            )
            .await
            .map_err(|e| {
                StoreError::P2p(format!(
                    "failed to download {} ({}): {e}",
                    store_key, candidate.info.info.root_hash
                ))
            })?;

        // Validate manifest inside downloaded HashSeq before materializing or recursing.
        let manifest = self
            .blob_store
            .load_manifest(candidate.info.info.root_hash)
            .await?;
        if manifest.store_key != store_key {
            visiting.remove(store_key);
            return Err(StoreError::InvalidManifest(format!(
                "downloaded manifest store_key '{}' does not match requested '{store_key}'",
                manifest.store_key
            )));
        }

        // Recursively substitute all referenced store paths before materializing this path.
        for dep in &manifest.references {
            if dep == store_key {
                continue;
            }
            let ok = Box::pin(self.substitute_recursive(store_dir, dep, visiting)).await?;
            if !ok {
                visiting.remove(store_key);
                return Err(StoreError::NotFound(format!(
                    "missing dependency '{dep}' required by '{store_key}'"
                )));
            }
        }

        self.blob_store.register_fetched(&candidate.info).await?;
        self.blob_store
            .materialize_path(store_key, &dest_path)
            .await?;

        visiting.remove(store_key);
        Ok(true)
    }

    /// Fetch a store path directly by content-addressed BLAKE3 `root_hash` (no signature needed).
    pub async fn fetch_by_hash(
        &self,
        root_hash: Hash,
        providers: Vec<EndpointId>,
        store_dir: &Path,
    ) -> Result<PathBuf> {
        let mut all_providers = providers;
        for p in &self.config.peers {
            if !all_providers.contains(p) {
                all_providers.push(*p);
            }
        }
        for a in &self.config.known_addrs {
            if !all_providers.contains(&a.id) {
                all_providers.push(a.id);
            }
        }
        if all_providers.is_empty() {
            return Err(StoreError::P2p(
                "no providers or peers specified for hash fetch".to_string(),
            ));
        }

        self.downloader
            .download(HashAndFormat::hash_seq(root_hash), all_providers)
            .await
            .map_err(|e| StoreError::P2p(format!("failed to download {root_hash}: {e}")))?;

        let manifest = self.blob_store.load_manifest(root_hash).await?;
        let info = PathInfo::new(
            manifest.store_key.clone(),
            root_hash,
            manifest.total_size,
            manifest.references.clone(),
            manifest.drv_path.clone(),
        );
        let signed = SignedPathInfo::new(info);
        self.blob_store.register_fetched(&signed).await?;

        let dest = store_dir.join(&manifest.store_key);
        self.blob_store
            .materialize_path(&manifest.store_key, &dest)
            .await?;
        Ok(dest)
    }

    async fn discover_candidates(&self, store_key: &str) -> Vec<Candidate> {
        let mut initial_peers: Vec<EndpointId> = self.config.peers.clone();
        for addr in &self.config.known_addrs {
            if !initial_peers.contains(&addr.id) {
                initial_peers.push(addr.id);
            }
        }

        let mut seen_peers: HashSet<EndpointId> = initial_peers.iter().copied().collect();
        let mut candidates_by_root: HashMap<Hash, Candidate> = HashMap::new();
        let timeout = self.config.timeout;

        // 1. Query initial peers (LookupClosure + 1-hop GetPeers) and DHT concurrently.
        let p2p_fut = async {
            let mut extra_peers = Vec::new();
            for peer in initial_peers {
                let ep = self.node.endpoint();
                if let Ok(Ok(Response::Closure(items))) = tokio::time::timeout(
                    timeout,
                    rpc_call(
                        ep,
                        peer,
                        &Request::LookupClosure {
                            store_key: store_key.to_string(),
                        },
                    ),
                )
                .await
                {
                    for item in items {
                        if item.info.store_key == store_key {
                            merge_candidate(&mut candidates_by_root, item, peer, false);
                        }
                    }
                }
                if let Ok(Ok(Response::Peers(more))) = tokio::time::timeout(
                    Duration::from_secs(3),
                    rpc_call(ep, peer, &Request::GetPeers { limit: 32 }),
                )
                .await
                {
                    for p in more {
                        if seen_peers.insert(p) {
                            extra_peers.push(p);
                        }
                    }
                }
            }
            for peer in extra_peers {
                if let Ok(Ok(Response::Path(Some(item)))) = tokio::time::timeout(
                    timeout,
                    rpc_call(
                        self.node.endpoint(),
                        peer,
                        &Request::LookupPath {
                            store_key: store_key.to_string(),
                        },
                    ),
                )
                .await
                    && item.info.store_key == store_key
                {
                    merge_candidate(&mut candidates_by_root, item, peer, false);
                }
            }
        };

        let dht_fut = async {
            if !self.config.enable_dht || self.config.trusted_keys.is_empty() {
                return Vec::new();
            }
            let Ok(dht) = DhtClient::new_client() else {
                return Vec::new();
            };
            let mut results = Vec::new();
            for trusted_key in &self.config.trusted_keys {
                if let Ok(Ok(Some(rec))) =
                    tokio::time::timeout(timeout, dht.lookup_path(trusted_key, store_key)).await
                {
                    results.push((*trusted_key, rec));
                }
            }
            results
        };

        let (_, dht_records) = tokio::join!(p2p_fut, dht_fut);

        for (trusted_key, rec) in dht_records {
            let providers = rec.provider_ids();
            if providers.is_empty() {
                continue;
            }
            let root_hash = rec.root_hash();
            // Try fetching full SignedPathInfo from one of the DHT-listed providers, or synthesize a trusted PathInfo.
            let mut enriched = None;
            for provider in &providers {
                if let Ok(Ok(Response::Path(Some(info)))) = tokio::time::timeout(
                    Duration::from_secs(5),
                    rpc_call(
                        self.node.endpoint(),
                        *provider,
                        &Request::LookupPath {
                            store_key: store_key.to_string(),
                        },
                    ),
                )
                .await
                    && info.info.root_hash == root_hash
                    && info.info.store_key == store_key
                {
                    enriched = Some(info);
                    break;
                }
            }

            if let Some(info) = enriched {
                for provider in providers {
                    merge_candidate(&mut candidates_by_root, info.clone(), provider, true);
                }
            } else {
                let _ = trusted_key;
                let synthetic = SignedPathInfo::new(PathInfo::new(
                    store_key.to_string(),
                    root_hash,
                    rec.total_size,
                    Vec::new(),
                    None,
                ));
                for provider in providers {
                    merge_candidate(&mut candidates_by_root, synthetic.clone(), provider, true);
                }
            }
        }

        candidates_by_root.into_values().collect()
    }

    fn select_trusted_candidate(
        &self,
        store_key: &str,
        candidates: Vec<Candidate>,
    ) -> Result<Option<Candidate>> {
        if candidates.is_empty() {
            return Ok(None);
        }

        let mut saw_untrusted = false;
        for cand in &candidates {
            if cand.trusted_via_dht {
                return Ok(Some(cand.clone()));
            }
            if !self.config.trusted_keys.is_empty()
                && cand.info.verify_any(&self.config.trusted_keys).is_ok()
            {
                return Ok(Some(cand.clone()));
            }
            saw_untrusted = true;
        }

        if self.config.allow_untrusted {
            return Ok(candidates.into_iter().next());
        }

        if saw_untrusted {
            return Err(StoreError::UntrustedPath(store_key.to_string()));
        }
        Ok(None)
    }
}

fn merge_candidate(
    map: &mut HashMap<Hash, Candidate>,
    info: SignedPathInfo,
    provider: EndpointId,
    trusted_via_dht: bool,
) {
    let entry = map.entry(info.info.root_hash).or_insert_with(|| Candidate {
        info,
        providers: Vec::new(),
        trusted_via_dht,
    });
    if trusted_via_dht {
        entry.trusted_via_dht = true;
    }
    if !entry.providers.contains(&provider) {
        entry.providers.push(provider);
    }
}
