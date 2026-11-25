//! Custom Iroh ALPN protocol (`/rix/p2p/1`) for store path/closure lookup and peer exchange.

use crate::blob_store::BlobStore;
use crate::error::{Result, StoreError};
use crate::manifest::SignedPathInfo;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

/// ALPN identifier for the Rix P2P metadata and peer-exchange protocol.
pub const RIX_P2P_ALPN: &[u8] = b"/rix/p2p/1";

/// Protocol version reported in `Response::Pong`.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum framed message size (4 MiB).
const MAX_MSG_SIZE: usize = 4 * 1024 * 1024;

/// Maximum number of `SignedPathInfo` entries returned in a single closure response.
const MAX_CLOSURE_ENTRIES: usize = 4096;

/// Wire request messages over `/rix/p2p/1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Ping,
    LookupPath { store_key: String },
    LookupClosure { store_key: String },
    ListPaths { limit: u32, offset: u32 },
    GetPeers { limit: u32 },
    AnnounceSeeder,
}

/// Wire response messages over `/rix/p2p/1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Pong {
        version: u32,
        endpoint_id: EndpointId,
        seeding: bool,
    },
    Path(Option<SignedPathInfo>),
    Closure(Vec<SignedPathInfo>),
    Paths(Vec<String>),
    Peers(Vec<EndpointId>),
    Ack,
    Error(String),
}

/// Iroh [`ProtocolHandler`] serving `/rix/p2p/1` on an opt-in seeder node.
#[derive(Debug, Clone)]
pub struct RixP2pHandler {
    endpoint_id: EndpointId,
    blob_store: BlobStore,
    peers: Arc<RwLock<HashSet<EndpointId>>>,
    allowed_paths: Option<Arc<HashSet<String>>>,
}

impl RixP2pHandler {
    pub fn new(
        endpoint_id: EndpointId,
        blob_store: BlobStore,
        peers: Arc<RwLock<HashSet<EndpointId>>>,
        allowed_paths: Option<HashSet<String>>,
    ) -> Self {
        Self {
            endpoint_id,
            blob_store,
            peers,
            allowed_paths: allowed_paths.map(Arc::new),
        }
    }

    fn is_path_allowed(&self, store_key: &str) -> bool {
        match &self.allowed_paths {
            Some(set) => set.contains(store_key),
            None => true,
        }
    }

    async fn handle_request(&self, remote_id: EndpointId, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong {
                version: PROTOCOL_VERSION,
                endpoint_id: self.endpoint_id,
                seeding: true,
            },
            Request::LookupPath { store_key } => {
                if !self.is_path_allowed(&store_key) {
                    return Response::Path(None);
                }
                match self.blob_store.get_path_info(&store_key).await {
                    Ok(info) => Response::Path(info),
                    Err(e) => Response::Error(e.to_string()),
                }
            }
            Request::LookupClosure { store_key } => {
                if !self.is_path_allowed(&store_key) {
                    return Response::Closure(Vec::new());
                }
                match self.collect_closure(&store_key).await {
                    Ok(closure) => Response::Closure(closure),
                    Err(e) => Response::Error(e.to_string()),
                }
            }
            Request::ListPaths { limit, offset } => match self.blob_store.list_paths().await {
                Ok(mut paths) => {
                    if let Some(allowed) = &self.allowed_paths {
                        paths.retain(|p| allowed.contains(p));
                    }
                    let start = (offset as usize).min(paths.len());
                    let count = (limit as usize).min(1024);
                    let end = start.saturating_add(count).min(paths.len());
                    Response::Paths(paths[start..end].to_vec())
                }
                Err(e) => Response::Error(e.to_string()),
            },
            Request::GetPeers { limit } => {
                let cap = (limit as usize).min(256);
                let peers: Vec<EndpointId> = self
                    .peers
                    .read()
                    .iter()
                    .copied()
                    .filter(|id| *id != remote_id)
                    .take(cap)
                    .collect();
                Response::Peers(peers)
            }
            Request::AnnounceSeeder => {
                if remote_id != self.endpoint_id {
                    self.peers.write().insert(remote_id);
                }
                Response::Ack
            }
        }
    }

    async fn collect_closure(&self, root_key: &str) -> Result<Vec<SignedPathInfo>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut out = Vec::new();
        queue.push_back(root_key.to_string());

        while let Some(key) = queue.pop_front() {
            if !visited.insert(key.clone()) {
                continue;
            }
            if out.len() >= MAX_CLOSURE_ENTRIES {
                break;
            }
            if !self.is_path_allowed(&key) {
                continue;
            }
            if let Some(info) = self.blob_store.get_path_info(&key).await? {
                for dep in &info.info.references {
                    if !visited.contains(dep) {
                        queue.push_back(dep.clone());
                    }
                }
                out.push(info);
            }
        }
        Ok(out)
    }
}

impl ProtocolHandler for RixP2pHandler {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
        let remote_id = conn.remote_id();
        loop {
            let Ok((mut send, mut recv)) = conn.accept_bi().await else {
                break;
            };
            let req_bytes = match read_frame(&mut recv).await {
                Ok(b) => b,
                Err(_) => break,
            };
            let resp = match serde_json::from_slice::<Request>(&req_bytes) {
                Ok(req) => self.handle_request(remote_id, req).await,
                Err(e) => Response::Error(format!("invalid request: {e}")),
            };
            let resp_bytes = serde_json::to_vec(&resp).map_err(AcceptError::from_err)?;
            if write_frame(&mut send, &resp_bytes).await.is_err() {
                break;
            }
            let _ = send.finish();
        }
        Ok(())
    }
}

/// Issue a single RPC call to `peer_id` over `/rix/p2p/1`.
pub async fn rpc_call(endpoint: &Endpoint, peer_id: EndpointId, req: &Request) -> Result<Response> {
    let conn = endpoint
        .connect(peer_id, RIX_P2P_ALPN)
        .await
        .map_err(|e| StoreError::P2p(format!("connect to {peer_id} failed: {e}")))?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| StoreError::P2p(format!("open_bi to {peer_id} failed: {e}")))?;

    let req_bytes = serde_json::to_vec(req)
        .map_err(|e| StoreError::Serialization(format!("encode Request: {e}")))?;
    write_frame(&mut send, &req_bytes).await?;
    send.finish()
        .map_err(|e| StoreError::P2p(format!("finish send stream failed: {e}")))?;

    let resp_bytes = read_frame(&mut recv).await?;
    let resp: Response = serde_json::from_slice(&resp_bytes)
        .map_err(|e| StoreError::Serialization(format!("decode Response: {e}")))?;
    Ok(resp)
}

async fn write_frame(send: &mut iroh::endpoint::SendStream, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_MSG_SIZE {
        return Err(StoreError::P2p(format!(
            "message size {} exceeds max {MAX_MSG_SIZE}",
            payload.len()
        )));
    }
    let len = (payload.len() as u32).to_be_bytes();
    send.write_all(&len)
        .await
        .map_err(|e| StoreError::P2p(format!("write length prefix: {e}")))?;
    send.write_all(payload)
        .await
        .map_err(|e| StoreError::P2p(format!("write payload: {e}")))?;
    Ok(())
}

async fn read_frame(recv: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| StoreError::P2p(format!("read length prefix: {e}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MSG_SIZE {
        return Err(StoreError::P2p(format!(
            "incoming frame length {len} exceeds max {MAX_MSG_SIZE}"
        )));
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| StoreError::P2p(format!("read payload: {e}")))?;
    Ok(buf)
}
