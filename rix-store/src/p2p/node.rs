//! Iroh `Endpoint` lifecycle for non-seeding client mode and opt-in `SeederHandle` mode.

use crate::blob_store::BlobStore;
use crate::error::{Result, StoreError};
use crate::keys::to_mainline_signing_key;
use crate::p2p::dht::{DhtClient, DhtPathRecord};
use crate::p2p::protocol::{RIX_P2P_ALPN, Request, RixP2pHandler, rpc_call};
use iroh::address_lookup::memory::MemoryLookup;
use iroh::address_lookup::{DnsAddressLookup, PkarrResolver};
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey};
use iroh_blobs::BlobsProtocol;
use iroh_mainline_address_lookup::DhtAddressLookup;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio_util_compat::CancelFlag;

/// Republish interval for DHT content records while seeding.
const DHT_REPUBLISH_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Non-seeding Iroh P2P client node (ephemeral identity, no incoming protocol router).
#[derive(Debug)]
pub struct P2pNode {
    endpoint: Endpoint,
    memory_lookup: MemoryLookup,
}

impl P2pNode {
    /// Spawn a privacy-preserving client endpoint that does NOT seed or accept connections.
    pub async fn spawn_client(enable_dht: bool, use_relays: bool) -> Result<Self> {
        let memory_lookup = MemoryLookup::new();
        let mut builder = Endpoint::builder(presets::Minimal).address_lookup(memory_lookup.clone());

        if use_relays {
            builder = builder
                .relay_mode(RelayMode::Default)
                .address_lookup(PkarrResolver::n0_dns())
                .address_lookup(DnsAddressLookup::n0_dns());
        } else {
            builder = builder.relay_mode(RelayMode::Disabled);
        }

        if enable_dht {
            builder = builder.address_lookup(DhtAddressLookup::builder().no_publish());
        }

        let endpoint = builder
            .bind()
            .await
            .map_err(|e| StoreError::P2p(format!("failed to bind iroh client endpoint: {e}")))?;

        Ok(Self {
            endpoint,
            memory_lookup,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn memory_lookup(&self) -> &MemoryLookup {
        &self.memory_lookup
    }

    pub fn add_peer_addr(&self, addr: EndpointAddr) {
        self.memory_lookup.add_endpoint_info(addr);
    }

    pub async fn close(self) {
        self.endpoint.close().await;
    }

    /// Spawn an opt-in seeder node serving `iroh-blobs` and `/rix/p2p/1`.
    pub async fn spawn_seeder(
        secret_key: SecretKey,
        blob_store: BlobStore,
        enable_dht: bool,
        use_relays: bool,
        allowed_paths: Option<HashSet<String>>,
        bootstrap_peers: Vec<EndpointId>,
    ) -> Result<SeederHandle> {
        let memory_lookup = MemoryLookup::new();
        let mut builder = if use_relays {
            Endpoint::builder(presets::N0)
        } else {
            Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
        };
        builder = builder
            .secret_key(secret_key.clone())
            .address_lookup(memory_lookup.clone());

        if enable_dht {
            builder = builder.address_lookup(DhtAddressLookup::builder());
        }

        let endpoint = builder
            .bind()
            .await
            .map_err(|e| StoreError::P2p(format!("failed to bind iroh seeder endpoint: {e}")))?;

        let endpoint_id = endpoint.id();
        let peers = Arc::new(RwLock::new(HashSet::new()));
        for p in &bootstrap_peers {
            if *p != endpoint_id {
                peers.write().insert(*p);
            }
        }

        let rix_handler = RixP2pHandler::new(
            endpoint_id,
            blob_store.clone(),
            Arc::clone(&peers),
            allowed_paths.clone(),
        );
        let blobs_proto = BlobsProtocol::new(blob_store.store(), None);

        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs_proto)
            .accept(RIX_P2P_ALPN, rix_handler)
            .spawn();

        let cancel = CancelFlag::new();
        let dht_task = if enable_dht {
            let task_cancel = cancel.clone();
            let task_store = blob_store.clone();
            let task_key = secret_key.clone();
            let task_allowed = allowed_paths.clone();
            Some(tokio::spawn(async move {
                run_dht_announce_loop(task_key, endpoint_id, task_store, task_allowed, task_cancel)
                    .await;
            }))
        } else {
            None
        };

        // Best-effort PEX announcement to configured bootstrap peers.
        if !bootstrap_peers.is_empty() {
            let ep = endpoint.clone();
            let peer_set = Arc::clone(&peers);
            tokio::spawn(async move {
                for peer in bootstrap_peers {
                    if peer == endpoint_id {
                        continue;
                    }
                    let _ = rpc_call(&ep, peer, &Request::AnnounceSeeder).await;
                    if let Ok(crate::p2p::protocol::Response::Peers(more)) =
                        rpc_call(&ep, peer, &Request::GetPeers { limit: 64 }).await
                    {
                        let mut guard = peer_set.write();
                        for p in more {
                            if p != endpoint_id {
                                guard.insert(p);
                            }
                        }
                    }
                }
            });
        }

        Ok(SeederHandle {
            router,
            memory_lookup,
            peers,
            cancel,
            dht_task,
        })
    }
}

/// Handle to a running opt-in seeder node.
pub struct SeederHandle {
    router: Router,
    memory_lookup: MemoryLookup,
    peers: Arc<RwLock<HashSet<EndpointId>>>,
    cancel: CancelFlag,
    dht_task: Option<JoinHandle<()>>,
}

impl SeederHandle {
    pub fn endpoint(&self) -> &Endpoint {
        self.router.endpoint()
    }

    pub fn endpoint_id(&self) -> EndpointId {
        self.router.endpoint().id()
    }

    pub fn addr(&self) -> EndpointAddr {
        self.router.endpoint().addr()
    }

    pub fn memory_lookup(&self) -> &MemoryLookup {
        &self.memory_lookup
    }

    pub fn known_peers(&self) -> Vec<EndpointId> {
        self.peers.read().iter().copied().collect()
    }

    pub async fn shutdown(self) -> Result<()> {
        self.cancel.cancel();
        if let Some(handle) = self.dht_task {
            handle.abort();
            let _ = handle.await;
        }
        self.router
            .shutdown()
            .await
            .map_err(|e| StoreError::P2p(format!("router shutdown error: {e}")))?;
        Ok(())
    }
}

async fn run_dht_announce_loop(
    secret_key: SecretKey,
    endpoint_id: EndpointId,
    blob_store: BlobStore,
    allowed_paths: Option<HashSet<String>>,
    cancel: CancelFlag,
) {
    let Ok(dht) = DhtClient::new_client() else {
        return;
    };
    let signing_key = to_mainline_signing_key(&secret_key);

    loop {
        if cancel.is_cancelled() {
            break;
        }
        if let Ok(paths) = blob_store.list_paths().await {
            let seq = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(1);
            for key in paths {
                if cancel.is_cancelled() {
                    break;
                }
                if let Some(ref allowed) = allowed_paths
                    && !allowed.contains(&key)
                {
                    continue;
                }
                if let Ok(Some(info)) = blob_store.get_path_info(&key).await {
                    let rec = DhtPathRecord::new(
                        info.info.root_hash,
                        info.info.total_size,
                        [endpoint_id],
                    );
                    let _ = dht.publish_path(&signing_key, &key, &rec, seq).await;
                }
            }
        }

        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(DHT_REPUBLISH_INTERVAL) => {}
        }
    }
}

mod tokio_util_compat {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;

    #[derive(Clone, Debug)]
    pub struct CancelFlag {
        cancelled: Arc<AtomicBool>,
        notify: Arc<Notify>,
    }

    impl CancelFlag {
        pub fn new() -> Self {
            Self {
                cancelled: Arc::new(AtomicBool::new(false)),
                notify: Arc::new(Notify::new()),
            }
        }

        pub fn cancel(&self) {
            self.cancelled.store(true, Ordering::SeqCst);
            self.notify.notify_waiters();
        }

        pub fn is_cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }

        pub async fn cancelled(&self) {
            if self.is_cancelled() {
                return;
            }
            self.notify.notified().await;
        }
    }
}
