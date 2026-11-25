//! Mainline DHT (BEP 44) content discovery via `n0-mainline`.
//!
//! Each record is stored as a signed mutable item keyed by `(trusted_pubkey, salt)`
//! where `salt = "rix:v1:<32-char-nix-hash>"` (39 bytes <= 64-byte BEP 44 limit).
//! Because BEP 44 mutable items are signed by the publisher's Ed25519 key, third
//! parties cannot spoof or overwrite records belonging to a trusted key.

use crate::error::{Result, StoreError};
use crate::path::split_store_key;
use iroh::EndpointId;
use n0_mainline::{Dht, MutableItem, SigningKey};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Maximum BEP 44 mutable item payload size (BEP 44 limit is 1000 bytes).
const MAX_DHT_RECORD_BYTES: usize = 950;
/// Maximum number of provider `EndpointId`s stored in a single DHT record.
const MAX_DHT_PROVIDERS: usize = 16;

/// Compact content-discovery record stored as a BEP 44 mutable item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DhtPathRecord {
    pub version: u8,
    pub root_hash: [u8; 32],
    pub total_size: u64,
    pub providers: Vec<[u8; 32]>,
}

impl DhtPathRecord {
    pub fn new(
        root_hash: iroh_blobs::Hash,
        total_size: u64,
        providers: impl IntoIterator<Item = EndpointId>,
    ) -> Self {
        let mut provider_bytes = Vec::new();
        for id in providers {
            let b = *id.as_bytes();
            if !provider_bytes.contains(&b) {
                provider_bytes.push(b);
            }
            if provider_bytes.len() >= MAX_DHT_PROVIDERS {
                break;
            }
        }
        Self {
            version: 1,
            root_hash: *root_hash.as_bytes(),
            total_size,
            providers: provider_bytes,
        }
    }

    pub fn root_hash(&self) -> iroh_blobs::Hash {
        iroh_blobs::Hash::from(self.root_hash)
    }

    pub fn provider_ids(&self) -> Vec<EndpointId> {
        self.providers
            .iter()
            .filter_map(|b| iroh::PublicKey::from_bytes(b).ok())
            .collect()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut copy = self.clone();
        copy.providers.truncate(MAX_DHT_PROVIDERS);
        let bytes = postcard::to_allocvec(&copy).map_err(|e| {
            StoreError::Serialization(format!("postcard encode DhtPathRecord: {e}"))
        })?;
        if bytes.len() > MAX_DHT_RECORD_BYTES {
            return Err(StoreError::Dht(format!(
                "DhtPathRecord exceeds BEP 44 limit ({} > {MAX_DHT_RECORD_BYTES})",
                bytes.len()
            )));
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let rec: Self = postcard::from_bytes(bytes).map_err(|e| {
            StoreError::Serialization(format!("postcard decode DhtPathRecord: {e}"))
        })?;
        if rec.version != 1 {
            return Err(StoreError::Dht(format!(
                "unsupported DhtPathRecord version {}",
                rec.version
            )));
        }
        Ok(rec)
    }
}

/// Derive the BEP 44 salt (`<= 64` bytes) for a Nix store key `<32-char-hash>-<name>`.
pub fn dht_salt_for_store_key(store_key: &str) -> Result<Vec<u8>> {
    let (hash, _) = split_store_key(store_key)
        .ok_or_else(|| StoreError::InvalidPath(format!("invalid store_key '{store_key}'")))?;
    let salt = format!("rix:v1:{hash}");
    debug_assert!(salt.len() <= 64);
    Ok(salt.into_bytes())
}

/// Async wrapper around `n0_mainline::Dht` for BEP 44 path announcements and lookups.
#[derive(Debug, Clone)]
pub struct DhtClient {
    dht: Dht,
}

impl DhtClient {
    /// Start a client-mode Mainline DHT node (queries and signed puts, no server routing).
    pub fn new_client() -> Result<Self> {
        let dht = Dht::client()
            .map_err(|e| StoreError::Dht(format!("failed to start Mainline DHT client: {e}")))?;
        Ok(Self { dht })
    }

    /// Start a client-mode Mainline DHT node with custom bootstrap addresses (useful in tests).
    pub fn with_bootstrap(bootstrap: &[String]) -> Result<Self> {
        let mut builder = Dht::builder();
        if bootstrap.is_empty() {
            builder.no_bootstrap();
        } else {
            builder.bootstrap(bootstrap);
        }
        let dht = builder
            .build()
            .map_err(|e| StoreError::Dht(format!("failed to build Mainline DHT node: {e}")))?;
        Ok(Self { dht })
    }

    /// Publish or update a signed [`DhtPathRecord`] on the Mainline DHT for `store_key`.
    pub async fn publish_path(
        &self,
        signing_key: &SigningKey,
        store_key: &str,
        record: &DhtPathRecord,
        seq: i64,
    ) -> Result<()> {
        let salt = dht_salt_for_store_key(store_key)?;
        let payload = record.encode()?;
        let pubkey_bytes = signing_key.verifying_key().to_bytes();

        let existing = self
            .dht
            .get_mutable_most_recent(&pubkey_bytes, Some(&salt))
            .await
            .ok()
            .flatten();

        let (next_seq, cas) = match existing {
            Some(prev) => (seq.max(prev.seq() + 1), Some(prev.seq())),
            None => (seq.max(1), None),
        };

        let item = MutableItem::new(signing_key, &payload, next_seq, Some(&salt));
        self.dht
            .put_mutable(item, cas)
            .await
            .map_err(|e| StoreError::Dht(format!("put_mutable failed for {store_key}: {e}")))?;
        Ok(())
    }

    /// Query the Mainline DHT for a [`DhtPathRecord`] published by `trusted_key` for `store_key`.
    pub async fn lookup_path(
        &self,
        trusted_key: &iroh::PublicKey,
        store_key: &str,
    ) -> Result<Option<DhtPathRecord>> {
        let salt = dht_salt_for_store_key(store_key)?;
        let maybe_item = self
            .dht
            .get_mutable_most_recent(trusted_key.as_bytes(), Some(&salt))
            .await
            .map_err(|e| StoreError::Dht(format!("get_mutable_most_recent failed: {e}")))?;
        match maybe_item {
            Some(item) => Ok(Some(DhtPathRecord::decode(item.value())?)),
            None => Ok(None),
        }
    }

    /// Parse a z32/hex EndpointId string (utility for callers).
    pub fn parse_endpoint_id(s: &str) -> Result<EndpointId> {
        EndpointId::from_str(s.trim())
            .map_err(|e| StoreError::Crypto(format!("invalid EndpointId '{s}': {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::generate_ephemeral_identity;
    use crate::path::compute_store_hash;

    #[test]
    fn test_dht_record_size_and_salt() {
        let hash = compute_store_hash(b"hello-pkg");
        let store_key = format!("{hash}-hello-pkg-1.0");
        let salt = dht_salt_for_store_key(&store_key).unwrap();
        assert_eq!(salt.len(), 39);
        assert!(salt.len() <= 64);

        let providers: Vec<EndpointId> = (0..20)
            .map(|_| generate_ephemeral_identity().public())
            .collect();
        let rec = DhtPathRecord::new(iroh_blobs::Hash::new(b"root"), 123_456, providers);
        assert_eq!(rec.providers.len(), MAX_DHT_PROVIDERS);
        let encoded = rec.encode().unwrap();
        assert!(
            encoded.len() <= MAX_DHT_RECORD_BYTES,
            "encoded length {} > {MAX_DHT_RECORD_BYTES}",
            encoded.len()
        );
        let decoded = DhtPathRecord::decode(&encoded).unwrap();
        assert_eq!(decoded, rec);
    }
}
