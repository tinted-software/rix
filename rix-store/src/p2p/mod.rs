//! Iroh-based P2P discovery, seeding, and substitution for Rix.

pub mod dht;
pub mod node;
pub mod protocol;
pub mod substituter;

pub use dht::{DhtClient, DhtPathRecord, dht_salt_for_store_key};
pub use node::{P2pNode, SeederHandle};
pub use protocol::{RIX_P2P_ALPN, Request, Response, RixP2pHandler};
pub use substituter::{Substituter, SubstitutionConfig};
