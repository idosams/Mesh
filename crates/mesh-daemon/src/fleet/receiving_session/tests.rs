use super::*;
use crate::fleet::remote_admission::authentication::tests::Fixture;
use crate::fleet::{
    RemoteFrameReader, RemoteFrameWriter, RemoteInputChunk, RemoteInputEntry, RemoteLaunchOutcome,
    Runtime,
};
use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
use ed25519_dalek::{Signer as _, SigningKey};
use mesh_cas::{Blake3, ContentDigest};
use mesh_types::{PublicKey, Signature};
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;

const ALLOCATION: &str = "0123456789abcdef0123456789abcdef";
pub(in crate::fleet) struct Setup {
    pub(in crate::fleet) f: Fixture,
    pub(in crate::fleet) destination: RemoteInputDestination,
    pub(in crate::fleet) manifest: RemoteInputManifest,
    pub(in crate::fleet) bytes: Vec<u8>,
    pub(in crate::fleet) digest: Digest32,
}
impl Setup {
    pub(in crate::fleet) fn new() -> Self {
        let mut f = Fixture::new();
        for name in ["store", "allocations"] {
            fs::create_dir(f.path.join(name)).unwrap();
            fs::set_permissions(f.path.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let destination = RemoteInputDestination::admit(
            &f.path.join("store"),
            ProtectedWorkspaceRoot::inspect(&f.path.join("store")).unwrap(),
            &f.path.join("allocations"),
            ProtectedWorkspaceRoot::inspect(&f.path.join("allocations")).unwrap(),
            &[],
        )
        .unwrap();
        let bytes = b"private transfer content".to_vec();
        let digest = Blake3::digest_bytes(&bytes);
        let manifest = RemoteInputManifest::new(
            f.work.assignment.input,
            vec![RemoteInputEntry::File {
                path: "result.txt".into(),
                executable: false,
                digest,
                chunks: vec![RemoteInputChunk {
                    digest,
                    bytes: bytes.len() as u64,
                }],
            }],
        )
        .unwrap();
        f.work.assignment.bundle = manifest.bundle();
        Self {
            f,
            destination,
            manifest,
            bytes,
            digest,
        }
    }
    pub(in crate::fleet) fn session(&self) -> RemoteReceivingSession<'_> {
        RemoteReceivingSession::new(
            self.f.registry(),
            self.f.work.clone(),
            ALLOCATION,
            &self.destination,
        )
    }
    fn sign(
        &self,
        connection: &RemoteReceivingConnection<'_, '_>,
        runtime: &mut Runtime,
    ) -> Signature {
        // Exercise the actual canonical proof boundary instead of signing arbitrary peer bytes.
        let proof = RemoteAdmissionProof::decode(&connection.proof().unwrap().encode()).unwrap();
        let payload = proof
            .signing_payload_for(
                runtime,
                "lane",
                "run",
                &PublicKey::from_bytes(self.f.coordinator.verifying_key().to_bytes()),
                &PublicKey::from_bytes(self.f.worker.verifying_key().to_bytes()),
            )
            .unwrap();
        Signature::from_bytes(self.f.coordinator.sign(payload.as_bytes()).to_bytes())
    }
    pub(in crate::fleet) fn manifest_frame(&self) -> RemoteFrame {
        wire(RemoteFrame::Manifest(
            self.manifest.encoded().as_bytes().to_vec(),
        ))
    }
    pub(in crate::fleet) fn part(&self, offset: usize, end: usize) -> RemoteFrame {
        wire(RemoteFrame::Chunk {
            digest: self.digest,
            offset: offset as u64,
            final_part: end == self.bytes.len(),
            bytes: self.bytes[offset..end].to_vec(),
        })
    }
    pub(in crate::fleet) fn assert_empty_store(&self) {
        assert_eq!(fs::read_dir(self.f.path.join("store")).unwrap().count(), 0);
    }
}
fn wire(frame: RemoteFrame) -> RemoteFrame {
    let mut bytes = Vec::new();
    RemoteFrameWriter::new(&mut bytes)
        .write_frame(&frame)
        .unwrap();
    RemoteFrameReader::new(Cursor::new(bytes))
        .read_frame()
        .unwrap()
        .unwrap()
}

#[test]
fn disconnect_requires_new_proof_and_preserves_original_transfer_through_native_launch_intent() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    {
        let mut unauthenticated = session.connect().unwrap();
        assert!(unauthenticated.receive(s.manifest_frame()).is_err());
        assert!(unauthenticated.proof().is_err());
        s.assert_empty_store();
    }
    let signature;
    {
        let mut connection = session.connect().unwrap();
        signature = s.sign(&connection, &mut runtime);
        assert_eq!(
            connection.authenticate(&signature).unwrap(),
            RemoteReceivingAccess::Receiving
        );
        assert_eq!(
            connection.receive(s.manifest_frame()).unwrap(),
            RemoteReceivingProgress::Manifest
        );
        assert!(matches!(
            connection.receive(s.part(0, 7)).unwrap(),
            RemoteReceivingProgress::Chunk {
                offset: 7,
                complete: false,
                ..
            }
        ));
    }
    // Original supervisor owns storage even with no broker connection alive.
    assert!(NativeRemoteInputReceiver::new(
        &s.destination,
        s.manifest.clone(),
        &s.f.work.assignment
    )
    .is_err());
    {
        let mut stale = session.connect().unwrap();
        assert!(stale.authenticate(&signature).is_err());
        assert!(stale.receive(s.part(7, s.bytes.len())).is_err());
    }
    let (allocation, registry) = {
        let mut connection = session.connect().unwrap();
        assert!(connection.status(s.digest).is_err());
        let signature = s.sign(&connection, &mut runtime);
        assert_eq!(
            connection.authenticate(&signature).unwrap(),
            RemoteReceivingAccess::Receiving
        );
        assert_eq!(connection.status(s.digest).unwrap(), (7, false));
        connection.receive(s.manifest_frame()).unwrap();
        assert_eq!(connection.status(s.digest).unwrap(), (7, false));
        connection.receive(s.part(7, s.bytes.len())).unwrap();
        assert_eq!(
            connection.status(s.digest).unwrap(),
            (s.bytes.len() as u64, true)
        );
        let parts = connection.materialize().unwrap();
        assert!(connection.materialize().is_err());
        parts
    };
    assert!(session.connect().is_err());
    assert_eq!(
        fs::read(allocation.path().join("result.txt")).unwrap(),
        s.bytes
    );
    let workspace = allocation
        .into_worker_workspace(
            TrustedReviewers::default(),
            CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    let outcome = registry
        .reserve_launch(
            workspace,
            "codex",
            super::super::service::received_clock().unwrap(),
        )
        .unwrap();
    assert!(matches!(outcome, RemoteLaunchOutcome::Reserved(_)));
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
}

#[test]
fn abandoned_or_wrongly_signed_challenge_cannot_admit_but_does_not_destroy_native_registry() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let old = {
        let c = session.connect().unwrap();
        s.sign(&c, &mut runtime)
    };
    {
        let mut c = session.connect().unwrap();
        assert!(c.authenticate(&old).is_err());
    }
    {
        let mut c = session.connect().unwrap();
        let wrong = SigningKey::from_bytes(&[99; 32]);
        let signature = Signature::from_bytes(wrong.sign(b"unrelated").to_bytes());
        assert!(c.authenticate(&signature).is_err());
    }
    assert!(s.f.registry().receipts().unwrap().is_empty());
    s.assert_empty_store();
    let mut c = session.connect().unwrap();
    let signature = s.sign(&c, &mut runtime);
    assert_eq!(
        c.authenticate(&signature).unwrap(),
        RemoteReceivingAccess::Receiving
    );
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
}

#[test]
fn reopened_receipt_is_authenticated_facts_only_and_never_recreates_a_transfer_reservation() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    {
        let mut session = s.session();
        let mut c = session.connect().unwrap();
        let signature = s.sign(&c, &mut runtime);
        c.authenticate(&signature).unwrap();
    }
    let mut restarted = s.session();
    let mut c = restarted.connect().unwrap();
    let signature = s.sign(&c, &mut runtime);
    assert_eq!(
        c.authenticate(&signature).unwrap(),
        RemoteReceivingAccess::Retained
    );
    assert!(c.receive(s.manifest_frame()).is_err());
    assert!(c.materialize().is_err());
    s.assert_empty_store();
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
}

#[test]
fn unexpected_control_changed_manifest_and_chunk_before_manifest_refuse_without_store_creation() {
    for bad in [
        RemoteFrame::Control(b"spawn".to_vec()),
        RemoteFrame::Manifest(b"{}".to_vec()),
        RemoteFrame::Chunk {
            digest: Digest32::from_bytes([1; 32]),
            offset: 0,
            final_part: true,
            bytes: vec![1],
        },
    ] {
        let s = Setup::new();
        let mut runtime = s.f.runtime(true);
        let mut session = s.session();
        let mut c = session.connect().unwrap();
        let signature = s.sign(&c, &mut runtime);
        c.authenticate(&signature).unwrap();
        assert!(c.receive(bad).is_err());
        assert!(c.receive(s.manifest_frame()).is_err());
        s.assert_empty_store();
    }
}

#[test]
fn incomplete_transfer_retains_reservation_but_materialization_failure_never_regrants_it() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    {
        let mut c = session.connect().unwrap();
        let signature = s.sign(&c, &mut runtime);
        c.authenticate(&signature).unwrap();
        c.receive(s.manifest_frame()).unwrap();
        assert!(c.materialize().is_err());
        c.receive(s.part(0, s.bytes.len())).unwrap();
        let conflict =
            s.f.path
                .join("allocations")
                .join(format!("input-{ALLOCATION}"));
        fs::create_dir(&conflict).unwrap();
        fs::write(conflict.join("retain"), b"existing work").unwrap();
        assert!(c.materialize().is_err());
        assert_eq!(fs::read(conflict.join("retain")).unwrap(), b"existing work");
    }
    assert!(session.connect().is_err());
    assert_eq!(s.f.registry().receipts().unwrap().len(), 1);
}

#[test]
fn replacing_destination_after_authentication_refuses_before_receiving_or_acknowledgment() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let mut c = session.connect().unwrap();
    let signature = s.sign(&c, &mut runtime);
    c.authenticate(&signature).unwrap();
    fs::rename(s.f.path.join("store"), s.f.path.join("retained-store")).unwrap();
    fs::create_dir(s.f.path.join("store")).unwrap();
    fs::set_permissions(s.f.path.join("store"), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(c.receive(s.manifest_frame()).is_err());
    assert_eq!(
        fs::read_dir(s.f.path.join("retained-store"))
            .unwrap()
            .count(),
        0
    );
    s.assert_empty_store();
}

#[test]
fn lease_expired_after_authentication_refuses_before_store_creation() {
    let s = Setup::new();
    let mut runtime = s.f.runtime(true);
    let mut session = s.session();
    let mut c = session.connect().unwrap();
    let signature = s.sign(&c, &mut runtime);
    c.authenticate(&signature).unwrap();
    // Move the native retained deadline behind the actual clock without waiting or weakening it.
    c.session.work.assignment.lease_until_ms = 1;
    assert!(c.receive(s.manifest_frame()).is_err());
    s.assert_empty_store();
}
