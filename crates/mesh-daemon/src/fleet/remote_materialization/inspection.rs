//! Read-only inspection of a ledger-bound acknowledged allocation after native restart.
use super::*;
use crate::fleet::RemoteMaterializationReceipt;

fn manifest_record(root: &PinnedWorkspaceRoot) -> io::Result<String> {
    let file = root
        .filesystem()
        .read_only()
        .read_file(Path::new("manifest.json"))?;
    let metadata = file.metadata()?;
    let maximum = crate::fleet::remote_input::MAX_MANIFEST_BYTES as u64;
    if metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > maximum
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(invalid());
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}
impl RemoteInputDestination {
    /// Verify the exact recorded input without initializing a workspace or recreating ownership.
    /// A successful observation says nothing about a provider process, current lease or safe retry.
    /// Missing, replaced, incomplete or changed storage refuses and is preserved for reconciliation.
    pub fn inspect_materialization(
        &self,
        receipt: &RemoteMaterializationReceipt,
    ) -> io::Result<()> {
        self.verify()?;
        let identities = receipt.directory_identities();
        let expected =
            |index: usize| ProtectedWorkspaceRoot::from_directory_token(identities[index].as_str());
        self.parent.ensure_protected_identity(expected(0)?)?;
        let name = format!("input-{}", receipt.admission().allocation());
        let allocation = self.parent.open_child_directory(OsStr::new(&name))?;
        allocation.ensure_protected_identity(expected(1)?)?;
        private(&allocation)?;
        let files = allocation.open_child_directory(OsStr::new("files"))?;
        files.ensure_protected_identity(expected(2)?)?;
        private(&files)?;
        let bytes = manifest_record(&allocation)?;
        let assignment = &receipt.admission().work().assignment;
        let manifest = RemoteInputManifest::decode(&bytes, assignment.input, assignment.bundle)
            .map_err(|_| invalid())?;
        // Deliberately no admission reservation: even an internal handle cannot initialize work.
        let input = RemoteInputAllocation {
            admission: None,
            manifest,
            parent: self.parent.clone(),
            allocation,
            files,
            path: self.parent_path.join(name).join("files"),
            protected: self.protected.clone(),
        };
        if input.retained_identity()? != *identities || manifest_record(&input.allocation)? != bytes
        {
            return Err(invalid());
        }
        self.verify()?;
        input.verify_roots()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::receiving_session::tests::Setup;
    use crate::fleet::{NativeRemoteInputReceiver, RemoteAdmissionOutcome};
    const ID: &str = "0123456789abcdef0123456789abcdef";
    fn recorded() -> (Setup, RemoteMaterializationReceipt) {
        let setup = Setup::new();
        let mut registry = setup.f.registry();
        let RemoteAdmissionOutcome::Reserved(reservation) = registry
            .reserve(
                setup.f.work.clone(),
                ID,
                crate::fleet::service::received_clock().unwrap(),
            )
            .unwrap()
        else {
            panic!("fresh reservation")
        };
        let mut receiver = NativeRemoteInputReceiver::new(
            &setup.destination,
            setup.manifest.clone(),
            &setup.f.work.assignment,
        )
        .unwrap();
        receiver
            .accept(setup.digest, 0, &setup.bytes, true)
            .unwrap();
        let allocation = receiver.materialize_reserved(reservation).unwrap();
        registry.retain_materialization(&allocation).unwrap();
        drop(allocation);
        drop(receiver);
        drop(registry);
        let receipt = setup
            .f
            .registry()
            .materialization_receipt("assignment")
            .unwrap()
            .unwrap();
        (setup, receipt)
    }
    fn reopen(setup: &Setup) -> RemoteInputDestination {
        RemoteInputDestination::admit(
            &setup.f.path.join("store"),
            ProtectedWorkspaceRoot::inspect(&setup.f.path.join("store")).unwrap(),
            &setup.f.path.join("allocations"),
            ProtectedWorkspaceRoot::inspect(&setup.f.path.join("allocations")).unwrap(),
            &[],
        )
        .unwrap()
    }
    #[test]
    fn restart_inspection_verifies_acknowledged_input_without_initialization_or_launch() {
        let (setup, receipt) = recorded();
        let root = setup.f.path.join("allocations").join(format!("input-{ID}"));
        for _ in 0..2 {
            reopen(&setup).inspect_materialization(&receipt).unwrap();
        }
        assert_eq!(
            fs::read(root.join("files/result.txt")).unwrap(),
            setup.bytes
        );
        assert!(!root.join("initialization.json").exists());
        assert!(!root.join("workspace.json").exists());
        assert!(!root.join("workspace.mesh").exists());
        assert!(setup
            .f
            .registry()
            .launch_receipt("assignment")
            .unwrap()
            .is_none());
    }
    #[test]
    fn replacement_allocation_or_files_with_identical_bytes_is_not_adopted() {
        for replace_allocation in [false, true] {
            let (setup, receipt) = recorded();
            let root = setup.f.path.join("allocations").join(format!("input-{ID}"));
            let target = if replace_allocation {
                root.clone()
            } else {
                root.join("files")
            };
            let retained = target.with_extension("retained");
            fs::rename(&target, &retained).unwrap();
            fs::create_dir(&target).unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
            let files = if replace_allocation {
                fs::copy(retained.join("manifest.json"), root.join("manifest.json")).unwrap();
                fs::create_dir(root.join("files")).unwrap();
                fs::set_permissions(root.join("files"), fs::Permissions::from_mode(0o700)).unwrap();
                root.join("files")
            } else {
                target
            };
            fs::write(files.join("result.txt"), &setup.bytes).unwrap();
            assert!(reopen(&setup).inspect_materialization(&receipt).is_err());
            assert!(retained.exists());
            assert_eq!(fs::read(files.join("result.txt")).unwrap(), setup.bytes);
        }
    }
    #[test]
    fn changed_input_or_invalid_manifest_refuses_without_repair() {
        for change in 0..5 {
            let (setup, receipt) = recorded();
            let root = setup.f.path.join("allocations").join(format!("input-{ID}"));
            match change {
                0 => fs::write(root.join("files/result.txt"), b"changed").unwrap(),
                1 => fs::write(root.join("manifest.json"), b"{}").unwrap(),
                2 => fs::set_permissions(
                    root.join("manifest.json"),
                    fs::Permissions::from_mode(0o644),
                )
                .unwrap(),
                3 => fs::hard_link(root.join("manifest.json"), root.join("manifest-copy")).unwrap(),
                _ => fs::write(root.join("files/extra"), b"preserved").unwrap(),
            }
            assert!(reopen(&setup).inspect_materialization(&receipt).is_err());
            assert!(root.exists());
            assert!(!root.join("initialization.json").exists());
            assert!(setup
                .f
                .registry()
                .launch_receipt("assignment")
                .unwrap()
                .is_none());
        }
    }
}
