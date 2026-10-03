//! Exercise authority changes at the actual worker signer boundary of the connected wire exchange.
use super::*;
fn exchange_fault(
    s: &Setup,
    runtime: &mut Runtime,
    registry: RemoteAdmissionRegistry,
    fault: impl FnMut(&SigningPayload) -> Result<Signature, String> + Send,
) -> (
    io::Result<RemoteRecoveryReceipt>,
    io::Result<RemoteRecoveryBrokerOutcome>,
) {
    let (client_stream, server) = pair();
    std::thread::scope(|scope| {
        let serving = scope.spawn(move || {
            let mut input = server.try_clone()?;
            let request = RemoteWorkerRecoveryRequest::decode(&control(
                &mut RemoteFrameReader::new(&mut input),
            )?)?
            .verify(&policy(s))?;
            serve_remote_recovery(
                RemoteRecoveryWorkerRequest {
                    request,
                    registry,
                    destination: &s.destination,
                    reviewers: crate::TrustedReviewers::default(),
                    checkpoint: crate::CheckpointRuntimeParameters::selected_defaults(),
                },
                input,
                server,
                fault,
            )
        });
        let reply = recover_remote_worker(
            client(s, runtime),
            client_stream.try_clone().unwrap(),
            client_stream,
            |p| sign(&s.f.coordinator, p),
        );
        (reply, serving.join().unwrap())
    })
}
fn assert_input_retained(s: &Setup) {
    assert_eq!(
        std::fs::read(
            s.f.path
                .join("allocations/input-0123456789abcdef0123456789abcdef/files/result.txt")
        )
        .unwrap(),
        s.bytes
    );
    assert!(guarded(s).launch_receipt("assignment").unwrap().is_none());
}
#[test]
fn signer_failure_or_wrong_key_before_recovery_preserves_original_input_without_initialization() {
    for wrong_key in [false, true] {
        let (s, mut runtime, registry) = ready();
        let before = runtime.state().clone();
        let (reply, worker) = exchange_fault(&s, &mut runtime, registry, |p| {
            if wrong_key {
                sign(&s.f.coordinator, p)
            } else {
                Err("custody unavailable".into())
            }
        });
        assert!(reply.is_err());
        assert!(worker.is_err());
        assert_eq!(runtime.state(), &before);
        unopened(&s);
        assert_input_retained(&s);
    }
}
#[test]
fn final_signer_failure_or_wrong_key_retains_exclusive_handoff_after_recovery() {
    for wrong_key in [false, true] {
        let (s, mut runtime, registry) = ready();
        let mut calls = 0;
        let (reply, outcome) = exchange_fault(&s, &mut runtime, registry, |p| {
            calls += 1;
            if calls == 1 {
                return sign(&s.f.worker, p);
            }
            if wrong_key {
                sign(&s.f.coordinator, p)
            } else {
                Err("custody unavailable".into())
            }
        });
        assert_eq!(calls, 2);
        assert!(reply.is_err());
        let outcome = outcome.unwrap();
        assert!(!outcome.reply_written);
        outcome.handoff.workspace.verify().unwrap();
        let (repeat_reply, repeat_worker) =
            exchange_fault(&s, &mut runtime, guarded(&s), |p| sign(&s.f.worker, p));
        assert!(repeat_reply.is_err());
        assert!(repeat_worker.is_err());
        outcome.handoff.workspace.verify().unwrap();
        assert_input_retained(&s);
    }
}
#[test]
fn worker_lease_change_during_signing_refuses_old_proof_or_retains_completed_handoff() {
    for phase in [1, 2] {
        let (s, mut runtime, registry) = ready();
        let mut calls = 0;
        let (reply, outcome) = exchange_fault(&s, &mut runtime, registry, |p| {
            calls += 1;
            if calls == phase {
                let mut update = guarded(&s);
                let admission = update.receipts().unwrap().remove(0);
                let lease = update.effective_lease(&admission).unwrap();
                update
                    .renew_lease(
                        &admission,
                        lease.sequence,
                        lease.until_ms + 30_000,
                        now().unwrap(),
                        180_000,
                    )
                    .unwrap();
            }
            sign(&s.f.worker, p)
        });
        assert!(reply.is_err());
        assert_eq!(calls, phase);
        if phase == 1 {
            assert!(outcome.is_err());
            unopened(&s);
        } else {
            let outcome = outcome.unwrap();
            assert!(!outcome.reply_written);
            outcome.handoff.workspace.verify().unwrap();
            assert!(outcome
                .handoff
                .registry
                .launch_receipt("assignment")
                .unwrap()
                .is_none());
        }
        assert_input_retained(&s);
    }
}
