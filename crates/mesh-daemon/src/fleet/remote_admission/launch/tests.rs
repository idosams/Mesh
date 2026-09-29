use super::*;
use crate::fleet::{NativeRemoteInputReceiver, RemoteInputDestination, RemoteInputManifest};
use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Barrier,
};

const ALLOCATION: &str = "0123456789abcdef0123456789abcdef";
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(in crate::fleet) struct Fixture(pub(in crate::fleet) PathBuf);
impl Fixture {
    pub(in crate::fleet) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mesh-launch-ownership-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        for name in ["store", "allocations", "worker"] {
            fs::create_dir(path.join(name)).unwrap();
            fs::set_permissions(path.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
    pub(in crate::fleet) fn registry(&self) -> RemoteAdmissionRegistry {
        registry(FleetStore::open(self.0.join("worker.sqlite")).unwrap())
    }
    fn admit(&self) -> RemoteAdmissionReceipt {
        let mut registry = self.registry();
        match registry.reserve(work(), ALLOCATION, 100).unwrap() {
            RemoteAdmissionOutcome::Reserved(value) => value.receipt().clone(),
            RemoteAdmissionOutcome::Retained(value) => value,
        }
    }
    fn workspace(&self, registry: &mut RemoteAdmissionRegistry) -> ReceivedWorkerWorkspace {
        self.workspace_for(registry, work())
    }
    fn workspace_for(
        &self,
        registry: &mut RemoteAdmissionRegistry,
        work: RemoteWork,
    ) -> ReceivedWorkerWorkspace {
        let destination = RemoteInputDestination::admit(
            &self.0.join("store"),
            ProtectedWorkspaceRoot::inspect(&self.0.join("store")).unwrap(),
            &self.0.join("allocations"),
            ProtectedWorkspaceRoot::inspect(&self.0.join("allocations")).unwrap(),
            &[],
        )
        .unwrap();
        let mut receiver =
            NativeRemoteInputReceiver::new(&destination, manifest(), &work.assignment).unwrap();
        let RemoteAdmissionOutcome::Reserved(input) =
            registry.reserve(work, ALLOCATION, 100).unwrap()
        else {
            panic!("fixture needs original admission");
        };
        receiver
            .materialize_reserved(input)
            .unwrap()
            .into_worker_workspace(
                TrustedReviewers::default(),
                CheckpointRuntimeParameters::selected_defaults(),
            )
            .unwrap()
    }
    #[cfg(target_os = "macos")]
    pub(in crate::fleet) fn result_session_reservation(&self) -> Box<RemoteLaunchReservation> {
        let key = ed25519_dalek::SigningKey::from_bytes(&[0x73; 32]);
        let worker: String = key
            .verifying_key()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let mut registry = RemoteAdmissionRegistry::new(
            FleetStore::open(self.0.join("worker.sqlite")).unwrap(),
            &"ab".repeat(32),
            &worker,
            "objective",
            limits(),
        )
        .unwrap();
        let now = crate::fleet::service::received_clock().unwrap();
        let mut work = work();
        work.assignment.worker_key = worker;
        work.assignment.lease_until_ms = now + 120_000;
        let workspace = self.workspace_for(&mut registry, work);
        let RemoteLaunchOutcome::Reserved(reservation) =
            registry.reserve_launch(workspace, "codex", now).unwrap()
        else {
            panic!("original required");
        };
        reservation
    }
    pub(in crate::fleet) fn session_reservation(&self) -> Box<RemoteLaunchReservation> {
        self.session_reservation_with(self.registry())
    }
    pub(in crate::fleet) fn session_reservation_with(
        &self,
        mut registry: RemoteAdmissionRegistry,
    ) -> Box<RemoteLaunchReservation> {
        let now = crate::fleet::service::received_clock().unwrap();
        let mut work = work();
        work.assignment.lease_until_ms = now + 120_000;
        let workspace = self.workspace_for(&mut registry, work);
        let RemoteLaunchOutcome::Reserved(reservation) =
            registry.reserve_launch(workspace, "codex", now).unwrap()
        else {
            panic!("expected original reservation");
        };
        reservation
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn limits() -> Limits {
    Limits {
        lanes: 1,
        concurrency: 1,
        depth: 0,
        retries: 0,
    }
}
fn registry(store: FleetStore) -> RemoteAdmissionRegistry {
    RemoteAdmissionRegistry::new(
        store,
        &"ab".repeat(32),
        &"cd".repeat(32),
        "objective",
        limits(),
    )
    .unwrap()
}
fn manifest() -> RemoteInputManifest {
    RemoteInputManifest::new(RecordDigest::from_bytes([1; 32]), vec![]).unwrap()
}
fn work() -> RemoteWork {
    RemoteWork {
        lane: "lane".into(),
        run: "run".into(),
        provider: "codex".into(),
        goal: "Private work".into(),
        assignment: RemoteAssignment {
            id: "assignment".into(),
            worker_key: "cd".repeat(32),
            input: manifest().input(),
            bundle: manifest().bundle(),
            lease_sequence: 1,
            lease_until_ms: 1000,
        },
    }
}
fn proposed(admission: RemoteAdmissionReceipt) -> RemoteLaunchReceipt {
    RemoteLaunchReceipt {
        admission,
        owner: "ef".repeat(32),
        mapping: RecordDigest::from_bytes([3; 32]),
        initial: RecordDigest::from_bytes([4; 32]),
        installation: "native-installation".into(),
    }
}

#[test]
fn concurrent_identical_intents_grant_once_and_restart_only_recovers_facts() {
    let fixture = Fixture::new();
    let proposed = proposed(fixture.admit());
    let barrier = Arc::new(Barrier::new(2));
    let handles = [fixture.registry(), fixture.registry()].map(|mut registry| {
        let barrier = barrier.clone();
        let proposed = proposed.clone();
        std::thread::spawn(move || {
            barrier.wait();
            registry.claim_launch_record(proposed, 100).unwrap()
        })
    });
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.iter().filter(|(_, inserted)| *inserted).count(), 1);
    assert!(results[0].0 == results[1].0);
    let mut after_restart = proposed.clone();
    after_restart.owner = "12".repeat(32);
    let (retained, inserted) = fixture
        .registry()
        .claim_launch_record(after_restart, 5000)
        .unwrap();
    assert!(!inserted);
    assert!(retained == proposed);
}

#[test]
fn changed_workspace_or_admission_cannot_evict_an_existing_owner() {
    let fixture = Fixture::new();
    let original = proposed(fixture.admit());
    assert!(
        fixture
            .registry()
            .claim_launch_record(original.clone(), 100)
            .unwrap()
            .1
    );
    let mut variants = vec![];
    let mut value = original.clone();
    value.mapping = RecordDigest::from_bytes([9; 32]);
    variants.push(value);
    let mut value = original.clone();
    value.initial = RecordDigest::from_bytes([9; 32]);
    variants.push(value);
    let mut value = original.clone();
    value.installation.push('x');
    variants.push(value);
    let mut value = original.clone();
    value.admission.coordinator = "00".repeat(32);
    variants.push(value);
    let mut value = original.clone();
    value.admission.work.goal.push('x');
    variants.push(value);
    for value in variants {
        assert!(fixture.registry().claim_launch_record(value, 100).is_err());
    }
    assert!(
        fixture
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .unwrap()
            == original
    );
}

#[test]
fn invalid_clock_or_expired_initial_lease_cannot_claim() {
    for now in [0, 1000, 1001] {
        let fixture = Fixture::new();
        let proposed = proposed(fixture.admit());
        assert!(fixture
            .registry()
            .claim_launch_record(proposed, now)
            .is_err());
        assert!(fixture
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .is_none());
    }
}

#[test]
fn unknown_or_corrupt_launch_records_are_preserved_and_refused() {
    for mutation in 0..5 {
        let fixture = Fixture::new();
        let proposed = proposed(fixture.admit());
        let mut registry = fixture.registry();
        let encoded = registry.encode_launch(&proposed);
        let payload = match mutation {
            0 => encoded.replace(
                "mesh.remote-launch-intent/v1",
                "mesh.remote-launch-intent/v2",
            ),
            1 => encoded.replacen('{', "{\"unknown\":true,", 1),
            2 => encoded.replace("native-installation", ""),
            3 => encoded.replace(&proposed.owner, "not-an-owner"),
            _ => encoded.replace("\"admission_revision\":1", "\"admission_revision\":2"),
        };
        assert_ne!(payload, encoded);
        let stream = registry.launch_stream("assignment");
        registry
            .store
            .append(&stream, 0, "launch", &payload)
            .unwrap();
        assert!(registry.launch_receipt("assignment").is_err());
        assert_eq!(
            registry.store.events(&stream, 0, 2).unwrap()[0].payload,
            payload
        );
    }
}

#[test]
fn native_workspace_reservation_binds_mapping_and_retains_intent_after_drop() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry();
    let workspace = fixture.workspace(&mut registry);
    let initial = workspace.binding().starting_version().unwrap();
    let mapping = RecordDigest::from_bytes(
        *Blake3::digest_bytes(workspace.receipt().encode().as_bytes()).as_bytes(),
    );
    let RemoteLaunchOutcome::Reserved(reservation) =
        registry.reserve_launch(workspace, "codex", 100).unwrap()
    else {
        panic!("first claim needs a reservation");
    };
    reservation.verify(101).unwrap();
    assert_eq!(reservation.receipt().initial_operation(), initial);
    assert_eq!(reservation.receipt().workspace_mapping(), mapping);
    assert!(reservation.verify(1000).is_err());
    let receipt = reservation.receipt().clone();
    fs::write(
        fixture
            .0
            .join(format!("allocations/input-{ALLOCATION}/files/changed")),
        b"changed",
    )
    .unwrap();
    assert!(reservation.verify(101).is_err());
    drop(reservation);
    assert!(
        fixture
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .unwrap()
            == receipt
    );
}

#[test]
fn wrong_provider_or_registry_cannot_commit_native_launch_intent() {
    for wrong_provider in [true, false] {
        let fixture = Fixture::new();
        let mut registry = fixture.registry();
        let workspace = fixture.workspace(&mut registry);
        if !wrong_provider {
            registry = RemoteAdmissionRegistry::new(
                FleetStore::open(fixture.0.join("worker.sqlite")).unwrap(),
                &"00".repeat(32),
                &"cd".repeat(32),
                "objective",
                limits(),
            )
            .unwrap();
        }
        assert!(registry
            .reserve_launch(
                workspace,
                if wrong_provider { "claude" } else { "codex" },
                100
            )
            .is_err());
        assert!(fixture
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .is_none());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn reservation_retains_native_worker_directory_ownership() {
    use crate::fleet::NativeRemoteWorkerDirectory;
    let fixture = Fixture::new();
    let path = fixture.0.join("worker");
    let token = ProtectedWorkspaceRoot::inspect(&path).unwrap();
    let directory =
        NativeRemoteWorkerDirectory::create(&path, token, &"cd".repeat(32), &[]).unwrap();
    let mut registry = directory
        .registry(&"ab".repeat(32), "objective", limits())
        .unwrap();
    let workspace = fixture.workspace(&mut registry);
    let RemoteLaunchOutcome::Reserved(reservation) =
        registry.reserve_launch(workspace, "codex", 100).unwrap()
    else {
        panic!("expected original claim");
    };
    let receipt = reservation.receipt().clone();
    drop(directory);
    assert!(NativeRemoteWorkerDirectory::reopen(&path, token, &"cd".repeat(32), &[]).is_err());
    reservation.verify(101).unwrap();
    drop(reservation);
    let reopened =
        NativeRemoteWorkerDirectory::reopen(&path, token, &"cd".repeat(32), &[]).unwrap();
    assert!(
        reopened
            .registry(&"ab".repeat(32), "objective", limits())
            .unwrap()
            .launch_receipt("assignment")
            .unwrap()
            .unwrap()
            == receipt
    );
}

#[test]
fn lost_authority_after_durable_intent_grants_nothing_and_restart_does_not_regrant() {
    use mesh_store::fleet::{FleetStoreAuthority, FleetStoreError};
    use std::sync::Mutex;
    #[derive(Debug)]
    struct RevokeAfterCommit {
        observer: Mutex<FleetStore>,
        stream: String,
    }
    impl FleetStoreAuthority for RevokeAfterCommit {
        fn check(&self) -> Result<(), FleetStoreError> {
            if self
                .observer
                .lock()
                .unwrap()
                .events(&self.stream, 0, 1)?
                .is_empty()
            {
                Ok(())
            } else {
                Err(FleetStoreError::AuthorityChanged)
            }
        }
    }
    let fixture = Fixture::new();
    let proposed = proposed(fixture.admit());
    let authority = Arc::new(RevokeAfterCommit {
        observer: Mutex::new(FleetStore::open(fixture.0.join("worker.sqlite")).unwrap()),
        stream: fixture.registry().launch_stream("assignment"),
    });
    // Guarded opens require a native-resolved parent; macOS temp_dir may use /var's alias.
    // Keep the final database entry unresolved so SQLite still rejects symbolic ledger files.
    let path = fixture.0.canonicalize().unwrap().join("worker.sqlite");
    let mut guarded = registry(FleetStore::open_guarded(&path, false, authority).unwrap());
    assert!(matches!(
        guarded.claim_launch_record(proposed.clone(), 100),
        Err(Error::Store(FleetStoreError::AuthorityChanged))
    ));
    drop(guarded);
    let (receipt, inserted) = fixture
        .registry()
        .claim_launch_record(proposed.clone(), 5000)
        .unwrap();
    assert!(!inserted);
    assert!(receipt == proposed);
}

#[test]
fn renewed_lease_extends_only_the_original_launch_reservation() {
    let fixture = Fixture::new();
    let reservation = fixture.session_reservation();
    let original = reservation.receipt().clone();
    let expiry = original.admission().work().assignment.lease_until_ms;
    assert!(reservation.verify(expiry).is_err());
    let mut registry = fixture.registry();
    registry
        .renew_lease(original.admission(), 1, expiry + 1000, expiry - 1, 2000)
        .unwrap();
    reservation.verify(expiry + 1).unwrap();
    assert!(reservation.verify(expiry + 1000).is_err());
    let mut proposed = original.clone();
    proposed.owner = "12".repeat(32);
    let (retained, inserted) = registry.claim_launch_record(proposed, expiry + 1).unwrap();
    assert!(!inserted);
    assert!(retained == original);
    assert!(reservation.receipt() == &original);
}
