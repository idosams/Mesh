//! Exclusive installation beneath the required start fence. No ordinary admission is minted here.
use super::*;
pub(super) const OWNER_INTENT: &str = "consumption-owner.pending";
impl PreparedNativeConsumedStart {
    /// Install the exact signed entries under the same complete custody barrier as the start.
    /// Retry only adopts retained native identities and content; unknown work is preserved.
    /// The result is a start-record fact, not a consumed version or permission to run agents.
    /// Destination history, owner consumption and completion must still commit before admission.
    pub fn install_fenced_consumed_start(
        &self,
        storage: &AttachmentStorage,
        staged: &StagedNativeConsumedStart,
    ) -> io::Result<RecordDigest> {
        self.fence_and_install_with_io(storage, staged, true, |_, _, _| Ok(()), |f| f.sync_all())
    }

    pub(super) fn verify_install_names(&self) -> io::Result<()> {
        let top = self.top_entries();
        let names = self
            .destination
            .attachment
            .pinned
            .filesystem()
            .read_directory_names_bounded(Path::new(""), top.len())?;
        if names
            .iter()
            .any(|name| !top.iter().any(|(path, _)| name == OsStr::new(path)))
        {
            return Err(invalid("consumption destination contains unexpected work"));
        }
        Ok(())
    }

    pub(super) fn retain_owner_consumption_intent(
        &self,
        graph: &crate::project_attachment::NativeDependencyGraph,
        start: RecordDigest,
    ) -> io::Result<String> {
        let j = |d: RecordDigest| Json::text(d.to_hex());
        let body = Json::object([
            ("request", j(self.request)),
            ("grant", j(self.grant)),
            (
                "start",
                Json::Array(vec![
                    Json::Array(vec![
                        j(self.basis.destination.work()),
                        j(self.basis.destination.installation()),
                    ]),
                    j(self.operation()),
                ]),
            ),
            ("inputs", graph.consumption_inputs_json()),
        ]);
        let intent = Json::object([
            (
                "schema",
                Json::text("mesh.native-consumption-owner-intent/v1"),
            ),
            ("start", j(start)),
            ("closure", j(graph.digest())),
            ("body", body),
        ])
        .encode();
        match read_private_in_store(&self.destination.store, OWNER_INTENT) {
            Ok(current) if current == intent => {}
            Ok(_) => return Err(invalid("owner consumption intent differs")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                write(&self.destination.store, OWNER_INTENT, intent.as_bytes())?;
            }
            Err(e) => return Err(e),
        }
        self.destination
            .store
            .filesystem()
            .sync_file(Path::new(OWNER_INTENT))?;
        self.destination.store.sync()?;
        if read_private_in_store(&self.destination.store, OWNER_INTENT)? != intent {
            return Err(invalid("owner consumption intent changed"));
        }
        Ok(intent)
    }

    pub(super) fn install_entries(
        &self,
        staged: &StagedNativeConsumedStart,
        allocation: &PinnedWorkspaceRoot,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
        mut hook: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()> {
        guard
            .require_roots(&[
                self.destination.store.clone(),
                self.destination.attachment.pinned.clone(),
                allocation.clone(),
            ])
            .map_err(error)?;
        self.verify_install_names()?;
        let stage_bytes = read(&staged.root, RECEIPT, MAX_RECEIPT)?;
        if hash(&stage_bytes) != staged.receipt {
            return Err(invalid("installation stage changed"));
        }
        let stage =
            Json::parse(std::str::from_utf8(&stage_bytes).map_err(error)?).map_err(error)?;
        let Some(Json::Array(expected_entries)) = stage.get("entries") else {
            return Err(invalid("installation receipts missing"));
        };
        // Every receipt and all staged objects were authenticated before any entry is moved.
        let manifests = self
            .checkpoint
            .checkpoint
            .manifests
            .iter()
            .map(|m| (m.id, m))
            .collect::<BTreeMap<_, _>>();
        let objects = self
            .checkpoint
            .objects
            .iter()
            .map(|b| (hash(b), b.as_slice()))
            .collect::<BTreeMap<_, _>>();
        for (index, (path, entry)) in self.top_entries().into_iter().enumerate() {
            guard.ensure_current().map_err(error)?;
            self.verify_install_names()?;
            let recovery = staged
                .root
                .open_child_directory(OsStr::new(&format!("entry-{index}")))?;
            let receipt =
                String::from_utf8(read(&recovery, RECEIPT, MAX_RECEIPT)?).map_err(error)?;
            if expected_entries.get(index)
                != Some(&Json::object([
                    ("path", Json::text(path)),
                    ("receipt", Json::text(&receipt)),
                ]))
            {
                return Err(invalid("installation entry receipt changed"));
            }
            let durable = match entry {
                InitialEntry::File { manifest, .. } => RetainedAddition::resume(
                    self.destination.attachment.pinned.clone(),
                    PathBuf::from(path),
                    recovery,
                    self.content(*manifest, &manifests, &objects)?,
                    &receipt,
                    self.limits.file_bytes,
                )?
                .apply()?,
                InitialEntry::Directory => RetainedTreeAddition::resume(
                    self.destination.attachment.pinned.clone(),
                    PathBuf::from(path),
                    recovery,
                    &receipt,
                    self.entry_limits(),
                )?
                .apply()?,
            };
            if !durable {
                return Err(invalid("consumption installation durability uncertain"));
            }
            hook("installed-entry")?;
        }
        self.verify_install_names()?;
        self.entry_receipts(&staged.root, false, true, allocation, guard)?;
        if self
            .destination
            .attachment
            .pinned
            .filesystem()
            .read_directory_names_bounded(Path::new(""), self.top_entries().len())?
            .len()
            != self.top_entries().len()
        {
            return Err(invalid("consumption installation incomplete"));
        }
        guard.ensure_current().map_err(error)?;
        hook("installed")
    }
}

#[cfg(test)]
pub(super) fn assert_installation(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
    staged: &StagedNativeConsumedStart,
    receipt: RecordDigest,
) {
    let destination = prepared.destination.project().root();
    let journal = prepared
        .destination
        .metadata_path()
        .join(crate::RECORD_FILE_NAME);
    let before = fs::read(&journal).unwrap();
    let owner_journal = prepared.owner.metadata_path().join(crate::RECORD_FILE_NAME);
    let owner_before = fs::read(&owner_journal).unwrap();
    let unexpected = destination.join("editor-work");
    fs::write(&unexpected, b"preserve me").unwrap();
    assert!(prepared
        .install_fenced_consumed_start(storage, staged)
        .is_err());
    assert_eq!(fs::read(&unexpected).unwrap(), b"preserve me");
    assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
    fs::remove_file(unexpected).unwrap();
    let (stage_path, _pin) = staged.root.stable_namespace().unwrap();
    let extra = stage_path.join("entry-0/foreign");
    fs::write(&extra, b"preserve recovery work").unwrap();
    assert!(prepared
        .install_fenced_consumed_start(storage, staged)
        .is_err());
    assert_eq!(fs::read(&extra).unwrap(), b"preserve recovery work");
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    fs::remove_file(extra).unwrap();
    let mut installed = BTreeMap::new();
    for mode in [
        "install-partial",
        "install-lost",
        "install-complete",
        "install-complete",
    ] {
        let value = Json::object([
            ("storage", Json::text(storage.path.to_string_lossy())),
            ("owner", Json::text(prepared.owner.id())),
            ("source", Json::text(prepared.source.id())),
            ("destination", Json::text(prepared.destination.id())),
            ("version", Json::text(prepared.version.operation().to_hex())),
            ("request", Json::text(prepared.request.to_hex())),
            ("grant", Json::text(prepared.grant.to_hex())),
            ("stage", Json::text(staged.receipt.to_hex())),
            ("physical", Json::text(identity(&staged.root).unwrap())),
            ("operation", Json::text(prepared.operation().to_hex())),
            ("receipt", Json::text(receipt.to_hex())),
            ("mode", Json::text(mode)),
        ]);
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
            .env_remove("MESH_PRIVATE_STAGE_RESTART")
            .env("MESH_FENCED_START_RESTART", value.encode()).output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(if mode == "install-complete" { 0 } else { 75 }),
            "child {mode}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let now = fs::read_dir(destination)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                (entry.file_name(), (metadata.dev(), metadata.ino()))
            })
            .collect::<BTreeMap<_, _>>();
        for (name, old) in &installed {
            assert_eq!(now.get(name), Some(old));
        }
        installed = now;
        assert_eq!(
            installed.len(),
            if mode == "install-partial" {
                1
            } else {
                prepared.top_entries().len()
            }
        );
        assert_eq!(fs::read(&journal).unwrap(), before);
        assert_eq!(fs::read(&owner_journal).unwrap(), owner_before);
        assert!(prepared.destination.saved_versions().is_err());
    }
    let intent_path = prepared.destination.metadata_path().join(OWNER_INTENT);
    let intent = fs::read(&intent_path).unwrap();
    fs::write(&intent_path, b"foreign owner intent").unwrap();
    assert!(prepared
        .install_fenced_consumed_start(storage, staged)
        .is_err());
    assert_eq!(fs::read(&intent_path).unwrap(), b"foreign owner intent");
    fs::write(&intent_path, intent).unwrap();
    let file = destination.join("tree/run");
    let original = fs::read(&file).unwrap();
    fs::write(&file, b"editor change").unwrap();
    assert!(prepared
        .install_fenced_consumed_start(storage, staged)
        .is_err());
    assert_eq!(fs::read(&file).unwrap(), b"editor change");
    fs::write(&file, original).unwrap();
    assert_eq!(
        prepared
            .install_fenced_consumed_start(storage, staged)
            .unwrap(),
        receipt
    );
}
