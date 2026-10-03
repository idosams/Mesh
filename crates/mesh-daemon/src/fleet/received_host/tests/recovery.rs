use super::*;
use crate::fleet::receiving_session::tests::{
    recovery::{guarded, materialized},
    Setup,
};
use ed25519_dalek::Signer as _;
use std::sync::{atomic::AtomicBool, mpsc};

fn recovered(setup: &Setup) -> Box<RemoteRecoveredHandoff> {
    let mut runtime = setup.f.runtime(true);
    let (input, registry) = materialized(setup, &mut runtime);
    drop(input);
    let challenge = registry.recovery_challenge("assignment").unwrap();
    let signature = challenge
        .proof()
        .sign_for(
            &mut runtime,
            "lane",
            "run",
            &mesh_types::PublicKey::from_bytes(setup.f.coordinator.verifying_key().to_bytes()),
            &resident_key(setup),
            |payload| {
                Ok(mesh_types::Signature::from_bytes(
                    setup.f.coordinator.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    let (workspace, registry) = challenge
        .recover(
            &signature,
            &setup.destination,
            crate::TrustedReviewers::default(),
            crate::CheckpointRuntimeParameters::selected_defaults(),
        )
        .unwrap();
    Box::new(RemoteRecoveredHandoff {
        workspace,
        registry,
    })
}

#[test]
fn recovered_mailbox_start_survives_reply_loss_and_runs_only_once() {
    let setup = Setup::new();
    let fixture = Fixture::new();
    let handoff = recovered(&setup);
    let admission = handoff.workspace.admission().clone();
    let root = std::path::PathBuf::from(handoff.workspace.binding().root());
    let initial = handoff.workspace.binding().starting_version();
    let mut resident = ReceivedWorkerSupervisor::new(1, resident_key(&setup)).unwrap();
    let (control, mailbox) = ReceivedWorkerMailbox::bounded();
    let (observations, observed) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let stop_loop = stop.clone();
        let owner = scope.spawn(move || {
            resident.serve(&mailbox, &stop_loop, &observations);
            resident
        });
        let stop_guard = StopResident(stop.clone());
        let (reply, received) = mpsc::sync_channel(1);
        drop(received);
        control
            .try_send(ReceivedWorkerRequest::StartRecovered {
                handoff,
                launch: Box::new(resident_launch(&fixture, Arc::new(Signers))),
                reply,
            })
            .unwrap_or_else(|_| panic!("empty mailbox"));
        wait_started(&root);
        assert!(guarded(&setup).recovery_challenge("assignment").is_err());
        let registry = guarded(&setup);
        let receipt = registry.launch_receipt("assignment").unwrap().unwrap();
        assert_eq!(Some(receipt.initial_operation()), initial);
        assert!(receipt.admission() == &admission);
        drop(control);
        fs::write(root.join("finish"), b"finish").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let sample = observed
                .recv_timeout(
                    deadline
                        .checked_duration_since(Instant::now())
                        .expect("autonomous completion"),
                )
                .unwrap();
            if sample.iter().any(|entry| {
                entry.admission == admission
                    && entry.observation.as_ref().is_ok_and(|workers| {
                        workers.iter().any(|worker| worker.outcome == Some(true))
                    })
            }) {
                break;
            }
        }
        drop(stop_guard);
        let resident = owner.join().unwrap();
        assert_eq!(resident.admissions().len(), 1);
        assert!(resident.snapshot(&admission).is_ok());
        assert_eq!(
            fs::read_to_string(root.join("launches.txt")).unwrap(),
            "one\n"
        );
    });
}

#[test]
fn recovered_mailbox_backpressure_returns_the_same_exclusive_workspace() {
    use mpsc::TrySendError;
    let setup = Setup::new();
    let fixture = Fixture::new();
    let handoff = recovered(&setup);
    let admission = handoff.workspace.admission().clone();
    let mapping = handoff.workspace.receipt().encode();
    let (control, mailbox) = ReceivedWorkerMailbox::bounded();
    for _ in 0..32 {
        let (reply, _) = mpsc::sync_channel(1);
        control
            .try_send(ReceivedWorkerRequest::Snapshot {
                admission: admission.clone(),
                reply,
            })
            .unwrap_or_else(|_| panic!("within capacity"));
    }
    let (reply, _) = mpsc::sync_channel(1);
    let request = ReceivedWorkerRequest::StartRecovered {
        handoff,
        launch: Box::new(resident_launch(&fixture, Arc::new(Signers))),
        reply,
    };
    let Err(TrySendError::Full(request)) = control.try_send(request) else {
        panic!("return original request");
    };
    drop(mailbox);
    let Err(TrySendError::Disconnected(ReceivedWorkerRequest::StartRecovered { handoff, .. })) =
        control.try_send(request)
    else {
        panic!("return original workspace");
    };
    handoff.workspace.verify().unwrap();
    assert_eq!(handoff.workspace.receipt().encode(), mapping);
    assert!(handoff.workspace.admission() == &admission);
    assert!(handoff
        .registry
        .launch_receipt("assignment")
        .unwrap()
        .is_none());
    assert!(!std::path::Path::new(handoff.workspace.binding().root())
        .join("launches.txt")
        .exists());
}

#[test]
fn recovered_start_failure_retains_the_slot_and_single_launch_intent() {
    let setup = Setup::new();
    let fixture = Fixture::new();
    let handoff = recovered(&setup);
    let admission = handoff.workspace.admission().clone();
    let root = std::path::PathBuf::from(handoff.workspace.binding().root());
    let mut resident = ReceivedWorkerSupervisor::new(1, resident_key(&setup)).unwrap();
    assert!(resident
        .start_recovered(
            *handoff,
            resident_launch(&fixture, Arc::new(RefusingSigner))
        )
        .is_err());
    assert_eq!(resident.admissions().len(), 1);
    assert!(resident.admissions()[0] == admission);
    assert!(guarded(&setup)
        .launch_receipt("assignment")
        .unwrap()
        .is_some());
    assert!(guarded(&setup).recovery_challenge("assignment").is_err());
    assert!(!root.join("launches.txt").exists());
    assert_eq!(fs::read(root.join("result.txt")).unwrap(), setup.bytes);
}
