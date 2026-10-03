//! Explicit original initialization recovery without holding the service mutex over transport.
use super::*;
use crate::fleet::{
    recover_remote_worker, NativeSshDestination, RemoteRecoveryClientRequest, RemoteRecoveryReceipt,
};
use mesh_crypto::SigningPayload;
use mesh_types::{PublicKey, Signature};
use std::{io, time::Duration};

/// Native-selected original attempt and independently admitted execution identities.
pub struct RemoteHistoryRecoveryRequest<'a> {
    /// Exact existing lane.
    pub lane: &'a str,
    /// Exact current claimed run.
    pub run: &'a str,
    /// Native coordinator execution identity.
    pub coordinator: PublicKey,
    /// Independently authenticated worker identity.
    pub worker: PublicKey,
}
impl FleetHistory {
    /// Recover original acknowledged initialization over one configured, bounded SSH connection.
    /// Errors retain uncertainty: never retry dispatch, release capacity or infer provider completion.
    /// The worker may retain the handoff even when the final response cannot be accepted.
    pub fn recover_original_worker_over_ssh(
        &self,
        peer: &NativeSshDestination,
        request: RemoteHistoryRecoveryRequest<'_>,
        budget: Duration,
        sign: impl FnMut(&SigningPayload) -> Result<Signature, String>,
    ) -> io::Result<RemoteRecoveryReceipt> {
        self.recover_original_worker(request, |request| {
            let mut connection = peer.connect(budget)?;
            let (input, output) = connection.streams()?;
            recover_remote_worker(request, input, output, sign)
        })
    }
    fn recover_original_worker<T>(
        &self,
        request: RemoteHistoryRecoveryRequest<'_>,
        exchange: impl FnOnce(RemoteRecoveryClientRequest<'_>) -> io::Result<T>,
    ) -> io::Result<T> {
        let unavailable = |_| io::Error::other("original recovery requires reconciliation");
        let mut runtime = {
            let inner = self.0.lock().map_err(unavailable)?;
            let store = inner
                .runtime
                .store
                .reopen_guarded_connection()
                .map_err(|_| io::Error::other("original recovery history unavailable"))?;
            Runtime::open(store, inner.runtime.objective())
                .map_err(|_| io::Error::other("original recovery history unavailable"))?
        };
        exchange(RemoteRecoveryClientRequest {
            runtime: &mut runtime,
            lane: request.lane,
            run: request.run,
            coordinator: request.coordinator,
            worker: request.worker,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::remote_admission::authentication::tests::Fixture;
    use ed25519_dalek::{Signer as _, SigningKey};
    struct NoAllocation;
    impl LaneAllocator for NoAllocation {
        fn allocate(&self, _: &str, _: &VersionInput) -> Result<LaneWorkspace, Unavailable> {
            panic!("original recovery cannot allocate another lane")
        }
    }
    #[derive(Debug)]
    struct Authority {
        path: std::path::PathBuf,
        file: std::fs::File,
    }
    impl mesh_store::fleet::FleetStoreAuthority for Authority {
        fn check(&self) -> Result<(), mesh_store::fleet::FleetStoreError> {
            use std::os::unix::fs::MetadataExt;
            let ok = (|| -> std::io::Result<bool> {
                let current = std::fs::symlink_metadata(&self.path)?;
                let held = self.file.metadata()?;
                Ok(current.is_file()
                    && !current.file_type().is_symlink()
                    && current.nlink() == 1
                    && (current.dev(), current.ino()) == (held.dev(), held.ino()))
            })()
            .unwrap_or(false);
            if ok {
                Ok(())
            } else {
                Err(mesh_store::fleet::FleetStoreError::AuthorityChanged)
            }
        }
    }
    fn guarded_runtime(f: &Fixture) -> Runtime {
        drop(f.runtime(true));
        let path = f.path.canonicalize().unwrap().join("coordinator.sqlite");
        let authority = Arc::new(Authority {
            file: std::fs::File::open(&path).unwrap(),
            path: path.clone(),
        });
        Runtime::open(
            mesh_store::fleet::FleetStore::open_guarded(&path, false, authority).unwrap(),
            "objective",
        )
        .unwrap()
    }
    fn public(k: &SigningKey) -> PublicKey {
        PublicKey::from_bytes(k.verifying_key().to_bytes())
    }
    #[test]
    fn recovery_releases_service_lock_and_preserves_history_after_disconnect() {
        let f = Fixture::new();
        let h = FleetHistory(Arc::new(
            FleetService::new(
                guarded_runtime(&f),
                Arc::new(NoAllocation),
                ["codex".into()].into(),
            )
            .unwrap(),
        ));
        let before = h.0.native_state().unwrap();
        let mut wire = Vec::new();
        let result = h.recover_original_worker(
            RemoteHistoryRecoveryRequest {
                lane: "lane",
                run: "run",
                coordinator: public(&f.coordinator),
                worker: public(&f.worker),
            },
            |request| {
                assert!(h.0.inner.try_lock().is_ok());
                recover_remote_worker(request, &b""[..], &mut wire, |p| {
                    assert!(h.0.inner.try_lock().is_ok());
                    assert_eq!(h.0.native_state().unwrap(), before);
                    Ok(Signature::from_bytes(
                        f.coordinator.sign(p.as_bytes()).to_bytes(),
                    ))
                })
            },
        );
        assert!(result.is_err());
        assert!(
            !wire.is_empty(),
            "request must reach the transport before disconnect"
        );
        assert_eq!(h.0.native_state().unwrap(), before);
    }
    #[test]
    fn cancellation_from_service_during_signing_prevents_recovery_request() {
        let f = Fixture::new();
        let h = FleetHistory(Arc::new(
            FleetService::new(
                guarded_runtime(&f),
                Arc::new(NoAllocation),
                ["codex".into()].into(),
            )
            .unwrap(),
        ));
        let mut wire = Vec::new();
        let result = h.recover_original_worker(
            RemoteHistoryRecoveryRequest {
                lane: "lane",
                run: "run",
                coordinator: public(&f.coordinator),
                worker: public(&f.worker),
            },
            |request| {
                recover_remote_worker(request, &b""[..], &mut wire, |p| {
                    h.0.native_command("cancel-recovery", Command::Cancel)
                        .unwrap();
                    Ok(Signature::from_bytes(
                        f.coordinator.sign(p.as_bytes()).to_bytes(),
                    ))
                })
            },
        );
        assert!(result.is_err());
        assert!(wire.is_empty());
        let state = h.0.native_state().unwrap();
        assert!(state.cancelled);
        assert_eq!(state.lanes["lane"].runs.len(), 1);
    }
}
