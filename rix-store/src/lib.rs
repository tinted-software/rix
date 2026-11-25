//! Iroh-blobs backed Nix-like store and P2P substituter for Rix.

pub mod blob_store;
pub mod config;
pub mod error;
pub mod keys;
pub mod manifest;
pub mod p2p;
pub mod path;
pub mod store;

pub use blob_store::{BlobStore, VerifyReport};
pub use config::{
    P2pConfig, RixConfig, default_blobs_dir, default_config_path, default_identity_path,
    default_store_dir, rix_home,
};
pub use error::{Result, StoreError};
pub use iroh_blobs::Hash as BlobHash;
pub use keys::{
    generate_ephemeral_identity, load_identity, load_or_generate_identity, parse_public_key,
    to_mainline_signing_key,
};
pub use manifest::{
    ManifestKind, PathInfo, PathSignature, SignedPathInfo, StoreManifest, TreeEntry,
};
pub use p2p::{
    DhtClient, DhtPathRecord, P2pNode, RIX_P2P_ALPN, Request, Response, RixP2pHandler,
    SeederHandle, Substituter, SubstitutionConfig, dht_salt_for_store_key,
};
pub use path::{
    compute_path_store_key, compute_store_hash, extract_store_key, is_valid_nix_hash,
    is_valid_store_key, nix_base32_encode, scan_references, split_store_key,
};
pub use store::{Store, block_on_store};
