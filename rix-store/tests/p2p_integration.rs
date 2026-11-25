//! End-to-end offline Iroh P2P integration tests for `rix-store`.

use bytes::Bytes;
use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{Connection, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, RelayMode};
use iroh_blobs::api::blobs::AddBytesOptions;
use iroh_blobs::hashseq::HashSeq;
use iroh_blobs::{BlobFormat, BlobsProtocol};
use rix_store::{
    BlobStore, ManifestKind, P2pNode, PathInfo, RIX_P2P_ALPN, Request, Response, SignedPathInfo,
    StoreError, StoreManifest, Substituter, SubstitutionConfig, compute_store_hash,
    generate_ephemeral_identity,
};
use std::path::Path;
use std::time::Duration;

fn set_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_p2p_signed_closure_substitution() {
    let tmp_a = tempfile::tempdir().unwrap();
    let store_dir_a = tmp_a.path().join("store");
    let blobs_dir_a = tmp_a.path().join("blobs");
    std::fs::create_dir_all(&store_dir_a).unwrap();

    // Create dependency `dep` and dependent `app` referencing `dep`
    let dep_key = format!("{}-dep-1.0", compute_store_hash(b"dep-1.0"));
    let dep_path = store_dir_a.join(&dep_key);
    std::fs::create_dir_all(dep_path.join("lib")).unwrap();
    std::fs::write(dep_path.join("lib/data.txt"), b"hello-from-dep").unwrap();

    let app_key = format!("{}-app-1.0", compute_store_hash(b"app-1.0"));
    let app_path = store_dir_a.join(&app_key);
    std::fs::create_dir_all(app_path.join("bin")).unwrap();
    std::fs::create_dir_all(app_path.join("empty")).unwrap();
    let script_path = app_path.join("bin/run");
    std::fs::write(
        &script_path,
        format!(
            "#!/bin/sh\ncat {}/{}/lib/data.txt\n",
            store_dir_a.display(),
            dep_key
        ),
    )
    .unwrap();
    set_exec(&script_path);
    std::fs::write(app_path.join("README.md"), b"app readme").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("run", app_path.join("bin/run-link")).unwrap();

    let bs_a = BlobStore::open_fs(&blobs_dir_a).await.unwrap();
    let sk_a = generate_ephemeral_identity();
    let dep_signed = bs_a
        .ingest_path(&store_dir_a, &dep_key, &dep_path, None, Some(&sk_a))
        .await
        .unwrap();
    let app_signed = bs_a
        .ingest_path(&store_dir_a, &app_key, &app_path, None, Some(&sk_a))
        .await
        .unwrap();
    assert_eq!(app_signed.info.references, vec![dep_key.clone()]);
    assert_eq!(
        dep_signed.verify_any(&[sk_a.public()]).unwrap(),
        sk_a.public()
    );

    // Start offline seeder Node A
    let seeder = P2pNode::spawn_seeder(sk_a.clone(), bs_a.clone(), false, false, None, vec![])
        .await
        .unwrap();

    // Node B trusts Node A's public key
    let tmp_b = tempfile::tempdir().unwrap();
    let store_dir_b = tmp_b.path().join("store");
    let blobs_dir_b = tmp_b.path().join("blobs");
    std::fs::create_dir_all(&store_dir_b).unwrap();
    let bs_b = BlobStore::open_fs(&blobs_dir_b).await.unwrap();

    let cfg = SubstitutionConfig {
        peers: vec![seeder.endpoint_id()],
        known_addrs: vec![seeder.addr()],
        trusted_keys: vec![sk_a.public()],
        enable_dht: false,
        use_relays: false,
        allow_untrusted: false,
        timeout: Duration::from_secs(10),
    };
    let sub = Substituter::new(bs_b.clone(), cfg).await.unwrap();
    let ok = sub.substitute_path(&store_dir_b, &app_key).await.unwrap();
    assert!(ok, "expected P2P substitution to succeed");

    // Verify BOTH `app` and its dependency `dep` were downloaded & materialized!
    let mat_dep = store_dir_b.join(&dep_key);
    let mat_app = store_dir_b.join(&app_key);
    assert_eq!(
        std::fs::read(mat_dep.join("lib/data.txt")).unwrap(),
        b"hello-from-dep"
    );
    assert_eq!(
        std::fs::read(mat_app.join("README.md")).unwrap(),
        b"app readme"
    );
    assert!(mat_app.join("empty").is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::symlink_metadata(mat_app.join("bin/run"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0);
        assert_eq!(
            std::fs::read_link(mat_app.join("bin/run-link")).unwrap(),
            Path::new("run")
        );
    }

    assert!(bs_b.has_path(&dep_key).await.unwrap());
    assert!(bs_b.has_path(&app_key).await.unwrap());

    sub.close().await;
    bs_b.shutdown().await.unwrap();
    seeder.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_p2p_untrusted_rejected_then_allow_untrusted_succeeds() {
    let tmp_a = tempfile::tempdir().unwrap();
    let store_dir_a = tmp_a.path().join("store");
    let blobs_dir_a = tmp_a.path().join("blobs");
    std::fs::create_dir_all(&store_dir_a).unwrap();

    let key = format!("{}-pkg-1.0", compute_store_hash(b"untrusted-pkg"));
    let pkg_path = store_dir_a.join(&key);
    std::fs::write(&pkg_path, b"single-file-payload").unwrap();

    let bs_a = BlobStore::open_fs(&blobs_dir_a).await.unwrap();
    let sk_a = generate_ephemeral_identity();
    bs_a.ingest_path(&store_dir_a, &key, &pkg_path, None, Some(&sk_a))
        .await
        .unwrap();

    let seeder = P2pNode::spawn_seeder(sk_a, bs_a.clone(), false, false, None, vec![])
        .await
        .unwrap();

    let tmp_b = tempfile::tempdir().unwrap();
    let store_dir_b = tmp_b.path().join("store");
    let blobs_dir_b = tmp_b.path().join("blobs");
    std::fs::create_dir_all(&store_dir_b).unwrap();
    let bs_b = BlobStore::open_fs(&blobs_dir_b).await.unwrap();

    // 1. No trusted_keys and allow_untrusted = false -> rejected
    let strict_cfg = SubstitutionConfig {
        peers: vec![seeder.endpoint_id()],
        known_addrs: vec![seeder.addr()],
        trusted_keys: vec![],
        enable_dht: false,
        use_relays: false,
        allow_untrusted: false,
        timeout: Duration::from_secs(10),
    };
    let sub_strict = Substituter::new(bs_b.clone(), strict_cfg).await.unwrap();
    let err = sub_strict
        .substitute_path(&store_dir_b, &key)
        .await
        .unwrap_err();
    assert!(
        matches!(err, StoreError::UntrustedPath(_)),
        "expected UntrustedPath error, got {err:?}"
    );
    assert!(!store_dir_b.join(&key).exists());
    sub_strict.close().await;

    // 2. allow_untrusted = true -> succeeds
    let permissive_cfg = SubstitutionConfig {
        peers: vec![seeder.endpoint_id()],
        known_addrs: vec![seeder.addr()],
        trusted_keys: vec![],
        enable_dht: false,
        use_relays: false,
        allow_untrusted: true,
        timeout: Duration::from_secs(10),
    };
    let sub_perm = Substituter::new(bs_b.clone(), permissive_cfg)
        .await
        .unwrap();
    let ok = sub_perm.substitute_path(&store_dir_b, &key).await.unwrap();
    assert!(ok);
    assert_eq!(
        std::fs::read(store_dir_b.join(&key)).unwrap(),
        b"single-file-payload"
    );

    sub_perm.close().await;
    bs_b.shutdown().await.unwrap();
    seeder.shutdown().await.unwrap();
}

#[derive(Debug, Clone)]
struct MaliciousHandler {
    canned: SignedPathInfo,
}

impl ProtocolHandler for MaliciousHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let (mut send, mut recv) = conn.accept_bi().await?;
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(AcceptError::from_err)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf)
            .await
            .map_err(AcceptError::from_err)?;
        let req: Request = serde_json::from_slice(&buf).map_err(AcceptError::from_err)?;
        let resp = match req {
            Request::LookupClosure { .. } => Response::Closure(vec![self.canned.clone()]),
            Request::LookupPath { .. } => Response::Path(Some(self.canned.clone())),
            Request::GetPeers { .. } => Response::Peers(vec![]),
            _ => Response::Ack,
        };
        let resp_bytes = serde_json::to_vec(&resp).map_err(AcceptError::from_err)?;
        send.write_all(&(resp_bytes.len() as u32).to_be_bytes())
            .await
            .map_err(AcceptError::from_err)?;
        send.write_all(&resp_bytes)
            .await
            .map_err(AcceptError::from_err)?;
        send.finish().map_err(AcceptError::from_err)?;
        conn.closed().await;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_p2p_rejects_tampered_signature_and_mismatched_manifest_key() {
    // Prepare a real HashSeq whose StoreManifest claims `other_key`, while PathInfo claims `requested_key`.
    let requested_key = format!("{}-requested", compute_store_hash(b"requested"));
    let other_key = format!("{}-other", compute_store_hash(b"other"));

    let bs_mal = BlobStore::open_mem();
    let payload_tt = bs_mal
        .store()
        .blobs()
        .add_slice(b"evil")
        .temp_tag()
        .await
        .unwrap();
    let manifest = StoreManifest {
        version: 1,
        store_key: other_key.clone(),
        kind: ManifestKind::File {
            executable: false,
            size: 4,
            blob: payload_tt.hash(),
        },
        total_size: 4,
        references: vec![],
        drv_path: None,
    };
    let manifest_tt = bs_mal
        .store()
        .blobs()
        .add_bytes(Bytes::from(serde_json::to_vec(&manifest).unwrap()))
        .temp_tag()
        .await
        .unwrap();
    let hs: HashSeq = [manifest_tt.hash(), payload_tt.hash()]
        .into_iter()
        .collect();
    let root_tt = bs_mal
        .store()
        .blobs()
        .add_bytes_with_opts(AddBytesOptions {
            data: hs.into_inner(),
            format: BlobFormat::HashSeq,
        })
        .temp_tag()
        .await
        .unwrap();

    let sk = generate_ephemeral_identity();
    let mut signed = SignedPathInfo::new(PathInfo::new(
        requested_key.clone(),
        root_tt.hash(),
        4,
        vec![],
        None,
    ));
    signed.sign(&sk).unwrap();
    // Corrupt the signature
    signed.signatures[0].signature = "00".repeat(64);

    let mem_lookup = MemoryLookup::new();
    let ep = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .secret_key(sk.clone())
        .address_lookup(mem_lookup)
        .bind()
        .await
        .unwrap();
    let mal_addr = ep.addr();
    let router = Router::builder(ep)
        .accept(iroh_blobs::ALPN, BlobsProtocol::new(bs_mal.store(), None))
        .accept(
            RIX_P2P_ALPN,
            MaliciousHandler {
                canned: signed.clone(),
            },
        )
        .spawn();

    let bs_client = BlobStore::open_mem();

    // 1. With trusted_keys = [sk.public()], corrupted signature must be rejected as UntrustedPath
    let cfg_trusted = SubstitutionConfig {
        peers: vec![mal_addr.id],
        known_addrs: vec![mal_addr.clone()],
        trusted_keys: vec![sk.public()],
        enable_dht: false,
        use_relays: false,
        allow_untrusted: false,
        timeout: Duration::from_secs(10),
    };
    let sub1 = Substituter::new(bs_client.clone(), cfg_trusted)
        .await
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let err1 = sub1
        .substitute_path(tmp.path(), &requested_key)
        .await
        .unwrap_err();
    assert!(matches!(err1, StoreError::UntrustedPath(_)), "got {err1:?}");
    sub1.close().await;

    // 2. Even with allow_untrusted = true, mismatched manifest.store_key must be rejected!
    let cfg_untrusted = SubstitutionConfig {
        peers: vec![mal_addr.id],
        known_addrs: vec![mal_addr],
        trusted_keys: vec![],
        enable_dht: false,
        use_relays: false,
        allow_untrusted: true,
        timeout: Duration::from_secs(10),
    };
    let sub2 = Substituter::new(bs_client.clone(), cfg_untrusted)
        .await
        .unwrap();
    let err2 = sub2
        .substitute_path(tmp.path(), &requested_key)
        .await
        .unwrap_err();
    assert!(
        matches!(err2, StoreError::InvalidManifest(_)),
        "expected InvalidManifest, got {err2:?}"
    );
    sub2.close().await;

    drop(root_tt);
    drop(manifest_tt);
    drop(payload_tt);
    router.shutdown().await.unwrap();
}
