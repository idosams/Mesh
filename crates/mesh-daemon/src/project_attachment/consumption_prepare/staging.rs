//! Durable private consumption material. Publication here never installs into the reserved root.
mod start_fence;
use super::*;
use crate::{
    managed_file::retained_replacement::{
        absent_parent, EntryLimits, RetainedAddition, RetainedTreeAddition, TreeInput,
    },
    project_attachment::consumption_plan::InitialEntry,
    root_authority::PinnedWorkspaceRoot,
};
use mesh_cas::DurableFs as _;
use std::{
    ffi::OsStr,
    fs,
    io::Read as _,
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
const RECEIPT: &str = "stage.json";
const MAX_RECEIPT: usize = 64 * 1024 * 1024;
static ATTEMPTS: AtomicU64 = AtomicU64::new(0);

/// An exact durable private stage. This is not a saved version, consumption receipt or run permit.
pub struct StagedNativeConsumedStart {
    root: PinnedWorkspaceRoot,
    receipt: RecordDigest,
    request: RecordDigest,
}
impl StagedNativeConsumedStart {
    /// Exact private staging evidence, to be retained by the required consumption transaction.
    pub fn receipt(&self) -> io::Result<RecordDigest> {
        if hash(&read(&self.root, RECEIPT, MAX_RECEIPT)?) != self.receipt {
            return Err(invalid("staged receipt changed"));
        }
        Ok(self.receipt)
    }
    /// Stable request identity; no consumption has been acknowledged.
    pub fn request(&self) -> RecordDigest {
        self.request
    }
}
fn identity(root: &PinnedWorkspaceRoot) -> io::Result<String> {
    root.ensure_namespace_identity()?;
    let (d, i) = root.identity()?;
    Ok(format!("{d:016x}:{i:016x}"))
}
fn write(root: &PinnedWorkspaceRoot, name: &str, bytes: &[u8]) -> io::Result<()> {
    root.filesystem()
        .write_new_file(Path::new(name), bytes, fs::Permissions::from_mode(0o600))?;
    root.filesystem().sync_file(Path::new(name))
}
fn read(root: &PinnedWorkspaceRoot, name: &str, limit: usize) -> io::Result<Vec<u8>> {
    root.ensure_namespace_identity()?;
    let mut file = root.filesystem().read_only().read_file(Path::new(name))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o077 != 0
        || metadata.len() > limit as u64
    {
        return Err(invalid("private consumption object is unsafe or oversized"));
    }
    let mut bytes = Vec::new();
    (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("private consumption object grew"));
    }
    root.ensure_namespace_identity()?;
    Ok(bytes)
}
impl PreparedNativeConsumedStart {
    fn content(
        &self,
        id: RecordDigest,
        manifests: &BTreeMap<RecordDigest, &mesh_store::ManifestRecord>,
        objects: &BTreeMap<RecordDigest, &[u8]>,
    ) -> io::Result<Vec<u8>> {
        let manifest = manifests
            .get(&id)
            .ok_or_else(|| invalid("staged manifest missing"))?;
        if manifest.byte_length > self.limits.file_bytes {
            return Err(invalid("staged file exceeds limit"));
        }
        let mut bytes = Vec::with_capacity(manifest.byte_length as usize);
        for chunk in &manifest.chunks {
            let value = objects
                .get(&chunk.digest)
                .ok_or_else(|| invalid("staged chunk missing"))?;
            if bytes.len() as u64 != chunk.byte_offset
                || value.len() as u64 != chunk.byte_length
                || bytes.len().saturating_add(value.len()) as u64 > manifest.byte_length
            {
                return Err(invalid("staged chunk layout changed"));
            }
            bytes.extend_from_slice(value);
        }
        if bytes.len() as u64 != manifest.byte_length || hash(&bytes) != manifest.content_digest {
            return Err(invalid("staged file content changed"));
        }
        Ok(bytes)
    }
    fn top_entries(&self) -> Vec<(&String, &InitialEntry)> {
        self.plan
            .entries
            .iter()
            .filter(|(path, _)| !path.contains('/'))
            .collect()
    }
    fn entry_limits(&self) -> EntryLimits {
        EntryLimits {
            entries: self.limits.entries + 1,
            bytes: self.limits.bytes,
            file_bytes: self.limits.file_bytes,
        }
    }
    fn entry_receipts(
        &self,
        root: &PinnedWorkspaceRoot,
        prepare: bool,
        allocation: &PinnedWorkspaceRoot,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<Vec<Json>> {
        guard
            .require_roots(&[
                self.destination.store.clone(),
                self.destination.attachment.pinned.clone(),
                allocation.clone(),
            ])
            .map_err(error)?;
        let mut receipts = Vec::new();
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
            let name = format!("entry-{index}");
            let recovery = if prepare {
                root.create_child_directory(OsStr::new(&name))?
            } else {
                root.open_child_directory(OsStr::new(&name))?
            };
            let receipt = if prepare {
                match entry {
                    InitialEntry::File {
                        manifest,
                        executable,
                    } => {
                        let parent =
                            absent_parent(&self.destination.attachment.pinned, Path::new(path))?
                                .ok_or_else(|| invalid("staging destination is occupied"))?;
                        RetainedAddition::prepare(
                            self.destination.attachment.pinned.clone(),
                            PathBuf::from(path),
                            parent,
                            self.content(*manifest, &manifests, &objects)?,
                            *executable,
                            recovery.clone(),
                            |_, _, _, _, _| Ok(()),
                        )?
                        .recovery_receipt()?
                    }
                    InitialEntry::Directory => {
                        let prefix = format!("{path}/");
                        let mut entries = Vec::new();
                        for (child, entry) in self
                            .plan
                            .entries
                            .range(prefix.clone()..)
                            .take_while(|(path, _)| path.starts_with(&prefix))
                        {
                            let Some(relative) = child.strip_prefix(&prefix) else {
                                continue;
                            };
                            entries.push(match entry {
                                InitialEntry::Directory => {
                                    TreeInput::Directory(relative.to_owned())
                                }
                                InitialEntry::File {
                                    manifest,
                                    executable,
                                } => TreeInput::File(
                                    relative.to_owned(),
                                    self.content(*manifest, &manifests, &objects)?,
                                    *executable,
                                ),
                            });
                        }
                        RetainedTreeAddition::prepare_bounded(
                            self.destination.attachment.pinned.clone(),
                            PathBuf::from(path),
                            &entries,
                            recovery.clone(),
                            self.entry_limits(),
                            |_, _, _, _| Ok(()),
                        )?
                        .recovery_receipt()?
                    }
                }
            } else {
                let raw = read(&recovery, RECEIPT, MAX_RECEIPT)?;
                let receipt = String::from_utf8(raw).map_err(error)?;
                match entry {
                    InitialEntry::File { manifest, .. } => {
                        RetainedAddition::resume(
                            self.destination.attachment.pinned.clone(),
                            PathBuf::from(path),
                            recovery.clone(),
                            self.content(*manifest, &manifests, &objects)?,
                            &receipt,
                            self.limits.file_bytes,
                        )?;
                    }
                    InitialEntry::Directory => {
                        RetainedTreeAddition::resume(
                            self.destination.attachment.pinned.clone(),
                            PathBuf::from(path),
                            recovery.clone(),
                            &receipt,
                            self.entry_limits(),
                        )?;
                    }
                }
                receipt
            };
            if let InitialEntry::File { executable, .. } = entry {
                let value = Json::parse(&receipt).map_err(error)?;
                let mode = value
                    .get("mode")
                    .and_then(Json::as_u64)
                    .ok_or_else(|| invalid("missing staged file mode"))?;
                if (mode & 0o111 != 0) != *executable {
                    return Err(invalid("staged executable state differs"));
                }
            }
            if matches!(entry, InitialEntry::Directory) {
                self.check_tree_receipt(path, &receipt, &manifests)?;
            }
            if prepare {
                write(&recovery, RECEIPT, receipt.as_bytes())?;
                recovery.sync()?;
            }
            if recovery
                .filesystem()
                .read_directory_names_bounded(Path::new(""), 2)?
                .len()
                != 2
            {
                return Err(invalid("entry recovery contains unexpected work"));
            }
            receipts.push(Json::object([
                ("path", Json::text(path)),
                ("receipt", Json::text(receipt)),
            ]));
        }
        Ok(receipts)
    }
    fn check_tree_receipt(
        &self,
        path: &str,
        raw: &str,
        manifests: &BTreeMap<RecordDigest, &mesh_store::ManifestRecord>,
    ) -> io::Result<()> {
        let json = Json::parse(raw).map_err(error)?;
        let Some(Json::Array(evidence)) = json.get("evidence") else {
            return Err(invalid("missing staged tree evidence"));
        };
        let prefix = format!("{path}/");
        let expected = self
            .plan
            .entries
            .range(prefix.clone()..)
            .take_while(|(p, _)| p.starts_with(&prefix))
            .map(|(p, e)| (&p[prefix.len()..], e))
            .collect::<BTreeMap<_, _>>();
        if evidence.len() != expected.len() + 1 {
            return Err(invalid("staged tree entry set differs from signed plan"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for item in evidence {
            let name = item
                .get("path")
                .and_then(Json::as_text)
                .ok_or_else(|| invalid("missing staged path"))?;
            if !seen.insert(name) {
                return Err(invalid("duplicate staged path"));
            }
            let entry = if name.is_empty() {
                &InitialEntry::Directory
            } else {
                expected
                    .get(name)
                    .copied()
                    .ok_or_else(|| invalid("unsigned staged path"))?
            };
            let kind = item.get("kind").and_then(Json::as_text);
            match entry {
                InitialEntry::Directory if kind == Some("directory") => {}
                InitialEntry::File {
                    manifest,
                    executable,
                } if kind == Some("file") => {
                    let manifest = manifests
                        .get(manifest)
                        .ok_or_else(|| invalid("missing staged manifest"))?;
                    let mode = item
                        .get("mode")
                        .and_then(Json::as_u64)
                        .ok_or_else(|| invalid("missing staged mode"))?;
                    if item.get("bytes").and_then(Json::as_u64) != Some(manifest.byte_length)
                        || item.get("digest").and_then(Json::as_text)
                            != Some(manifest.content_digest.to_hex().as_str())
                        || (mode & 0o111 != 0) != *executable
                    {
                        return Err(invalid("staged file differs from signed plan"));
                    }
                }
                _ => return Err(invalid("staged entry kind differs from signed plan")),
            }
        }
        Ok(())
    }
    fn attempt_manifest(&self, root: &PinnedWorkspaceRoot) -> io::Result<String> {
        Ok(Json::object([
            ("schema", Json::text("mesh.native-consumption-attempt/v1")),
            ("request", Json::text(self.request.to_hex())),
            ("operation", Json::text(self.operation().to_hex())),
            ("root", Json::text(identity(root)?)),
            (
                "destination",
                Json::text(identity(&self.destination.attachment.pinned)?),
            ),
            ("store", Json::text(identity(&self.destination.store)?)),
            ("grant", Json::text(self.grant.to_hex())),
            ("closure", Json::text(self.basis.graph.to_hex())),
            (
                "prospective",
                Json::text(hash(self.basis.prospective.as_bytes()).to_hex()),
            ),
        ])
        .encode())
    }
    fn stage_manifest(
        &self,
        root: &PinnedWorkspaceRoot,
        graph: &Json,
        entries: Vec<Json>,
    ) -> io::Result<String> {
        Ok(Json::object([
            ("schema", Json::text("mesh.native-consumption-stage/v1")),
            ("request", Json::text(self.request.to_hex())),
            (
                "attempt",
                Json::text(hash(self.attempt_manifest(root)?.as_bytes()).to_hex()),
            ),
            ("root", Json::text(identity(root)?)),
            (
                "destination",
                Json::text(identity(&self.destination.attachment.pinned)?),
            ),
            ("store", Json::text(identity(&self.destination.store)?)),
            ("grant", Json::text(self.grant.to_hex())),
            ("configuration", Json::text(&self.basis.configuration)),
            ("prospective", Json::text(&self.basis.prospective)),
            ("operation", Json::text(self.operation().to_hex())),
            ("graph", graph.clone()),
            ("frames", Json::text(hash(&self.frames()).to_hex())),
            (
                "objects",
                Json::Array(
                    self.checkpoint
                        .objects
                        .iter()
                        .map(|b| Json::text(hash(b).to_hex()))
                        .collect(),
                ),
            ),
            ("entries", Json::Array(entries)),
        ])
        .encode())
    }
    fn frames(&self) -> Vec<u8> {
        self.checkpoint
            .checkpoint
            .records()
            .iter()
            .flat_map(mesh_store::frame_record)
            .collect()
    }
    fn verify_stage(
        &self,
        root: &PinnedWorkspaceRoot,
        graph: &Json,
        allocation: &PinnedWorkspaceRoot,
        guard: &crate::workspace_custody::WorkspaceInitializationGuard,
    ) -> io::Result<RecordDigest> {
        if root.try_clone_directory()?.metadata()?.mode() & 0o077 != 0 {
            return Err(invalid("consumption bundle is not private"));
        }
        if read(root, "attempt.json", 65536)? != self.attempt_manifest(root)?.as_bytes() {
            return Err(invalid("consumption attempt identity differs"));
        }
        let receipts = self.entry_receipts(root, false, allocation, guard)?;
        let expected = self.stage_manifest(root, graph, receipts)?;
        if expected.len() > MAX_RECEIPT
            || read(root, RECEIPT, MAX_RECEIPT)? != expected.as_bytes()
            || read(root, "frames.mesh", 80 * 1024 * 1024)? != self.frames()
        {
            return Err(invalid("consumption stage intent differs"));
        }
        for bytes in &self.checkpoint.objects {
            if read(
                root,
                &format!("object-{}", hash(bytes).to_hex()),
                bytes.len(),
            )? != *bytes
            {
                return Err(invalid("consumption stage object differs"));
            }
        }
        let count = 3 + self.checkpoint.objects.len() + self.top_entries().len();
        if root
            .filesystem()
            .read_directory_names_bounded(Path::new(""), count)?
            .len()
            != count
        {
            return Err(invalid("consumption stage contains unexpected entries"));
        }
        Ok(hash(expected.as_bytes()))
    }
    /// Preserve complete private staging for this candidate. No destination file or journal changes,
    /// no consumption acknowledgement, and no continuing grant or permission to launch an agent.
    pub fn stage(&self, storage: &AttachmentStorage) -> io::Result<StagedNativeConsumedStart> {
        self.stage_with_hook(storage, |_, _| Ok(()))
    }
    fn stage_with_hook(
        &self,
        storage: &AttachmentStorage,
        mut hook: impl FnMut(&str, &PinnedWorkspaceRoot) -> io::Result<()>,
    ) -> io::Result<StagedNativeConsumedStart> {
        self.revalidate(storage)?;
        let available = self.available.iter().collect::<Vec<_>>();
        let graph = storage.inspect_dependency_graph(
            &self.owner,
            &self.source,
            self.version,
            &available,
        )?;
        if graph.digest() != self.basis.graph {
            return Err(invalid("staging source closure changed"));
        }
        let graph = Json::object([
            ("closure", graph.to_json()),
            ("retained", graph.retained_content_json()),
        ]);
        let origin = storage
            .lane_origin_bound(&self.destination)?
            .ok_or_else(|| invalid("staging reservation missing"))?;
        let target = format!("consumption-{}", self.request.to_hex());
        // Private immutable staging holds destination and allocation custody. It cannot extend
        // that set or call complete graph/grant revalidation until this guard has been released.
        let guard = crate::workspace_custody::lock_workspace_initialization_set(&[
            self.destination.store.clone(),
            self.destination.attachment.pinned.clone(),
            origin.allocation.clone(),
        ])
        .map_err(error)?;
        let root = match origin.allocation.open_child_directory(OsStr::new(&target)) {
            Ok(root) => root,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(error)?
                    .as_nanos();
                let name = format!(
                    "consumption-attempt-{}-{}-{stamp}-{}",
                    self.request.to_hex(),
                    std::process::id(),
                    ATTEMPTS.fetch_add(1, Ordering::Relaxed)
                );
                let root = origin
                    .allocation
                    .create_child_directory(OsStr::new(&name))?;
                write(
                    &root,
                    "attempt.json",
                    self.attempt_manifest(&root)?.as_bytes(),
                )?;
                root.sync()?;
                origin.allocation.sync()?;
                hook("attempt-created", &root)?;
                let receipts = self.entry_receipts(&root, true, &origin.allocation, &guard)?;
                hook("entries-staged", &root)?;
                for bytes in &self.checkpoint.objects {
                    write(&root, &format!("object-{}", hash(bytes).to_hex()), bytes)?;
                }
                write(&root, "frames.mesh", &self.frames())?;
                let manifest = self.stage_manifest(&root, &graph, receipts)?;
                if manifest.len() > MAX_RECEIPT {
                    return Err(invalid("consumption stage receipt exceeds bound"));
                }
                write(&root, RECEIPT, manifest.as_bytes())?;
                root.sync()?;
                hook("bundle-synced", &root)?;
                self.verify_stage(&root, &graph, &origin.allocation, &guard)?;
                guard.ensure_current().map_err(error)?;
                origin.allocation.publish_child_directory(
                    OsStr::new(&name),
                    &root,
                    &origin.allocation,
                    OsStr::new(&target),
                )?
            }
            Err(e) => return Err(e),
        };
        hook("published", &root)?;
        let receipt = self.verify_stage(&root, &graph, &origin.allocation, &guard)?;
        root.sync()?;
        origin.allocation.sync()?;
        guard.ensure_current().map_err(error)?;
        drop(guard);
        hook("custody-released", &root)?;
        // Recheck permission and the complete source basis after private work; this is not
        // authority for a later installation. The consuming commit needs its own full barrier.
        self.revalidate(storage)?;
        Ok(StagedNativeConsumedStart {
            root,
            receipt,
            request: self.request,
        })
    }
}

#[cfg(test)]
pub(super) fn assert_private_stage(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
) {
    let origin = storage
        .lane_origin_bound(&prepared.destination)
        .unwrap()
        .unwrap();
    let target = format!("consumption-{}", prepared.request.to_hex());
    let before = origin
        .allocation
        .filesystem()
        .read_directory_names(Path::new(""))
        .unwrap();
    {
        let incomplete = crate::workspace_custody::lock_workspace_initialization_set(&[
            prepared.destination.store.clone(),
            prepared.destination.attachment.pinned.clone(),
        ])
        .unwrap();
        assert!(prepared
            .entry_receipts(&origin.allocation, true, &origin.allocation, &incomplete)
            .is_err());
    }
    assert_eq!(
        origin
            .allocation
            .filesystem()
            .read_directory_names(Path::new(""))
            .unwrap(),
        before
    );
    let mut attempts = Vec::new();
    for boundary in ["attempt-created", "entries-staged", "bundle-synced"] {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let mut worker = None;
        assert!(prepared
            .stage_with_hook(storage, |step, root| {
                if step == boundary {
                    attempts.push(root.clone());
                    let roots = [
                        prepared.destination.store.clone(),
                        prepared.destination.attachment.pinned.clone(),
                        origin.allocation.clone(),
                    ];
                    let started = started_tx.clone();
                    let acquired = acquired_tx.clone();
                    worker = Some(std::thread::spawn(move || {
                        started.send(()).unwrap();
                        let _guard =
                            crate::workspace_custody::lock_workspace_initialization_set(&roots)
                                .unwrap();
                        acquired.send(()).unwrap();
                    }));
                    started_rx
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    assert!(
                        matches!(
                            acquired_rx.recv_timeout(std::time::Duration::from_millis(150)),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        ),
                        "concurrent native writer entered private staging"
                    );
                    return Err(io::Error::other("interrupted private construction"));
                }
                Ok(())
            })
            .is_err());
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        worker.unwrap().join().unwrap();
        assert!(origin
            .allocation
            .open_child_directory(OsStr::new(&target))
            .is_err());
        for attempt in &attempts {
            attempt.ensure_namespace_identity().unwrap();
        }
        assert_eq!(
            fs::read_dir(prepared.destination.project().root())
                .unwrap()
                .count(),
            0
        );
    }
    let launch = |mode: &str, expected: &str, physical: &str| {
        let input = Json::object([
            ("storage", Json::text(storage.path.to_str().unwrap())),
            ("owner", Json::text(prepared.owner.id())),
            ("source", Json::text(prepared.source.id())),
            ("destination", Json::text(prepared.destination.id())),
            ("version", Json::text(prepared.version.operation().to_hex())),
            ("request", Json::text(prepared.request.to_hex())),
            ("grant", Json::text(prepared.grant.to_hex())),
            ("mode", Json::text(mode)),
            ("receipt", Json::text(expected)),
            ("physical", Json::text(physical)),
        ])
        .encode();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "project_attachment::consumption_prepare::tests::saved_ignore_rules_bind_candidate_without_changing_empty_reservation", "--nocapture"])
            .env("MESH_PRIVATE_STAGE_RESTART", input).output().unwrap();
        if mode == "publish" {
            assert_eq!(result.status.code(), Some(75));
        } else {
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                String::from_utf8_lossy(&result.stdout).contains("exact private stage recovered")
            );
        }
    };
    launch("publish", "", "");
    let published = origin
        .allocation
        .open_child_directory(OsStr::new(&target))
        .unwrap()
        .identity()
        .unwrap();
    let staged = prepared.stage(storage).unwrap();
    assert_eq!(staged.root.identity().unwrap(), published);
    for _ in 0..2 {
        launch(
            "recover",
            &staged.receipt().unwrap().to_hex(),
            &identity(&staged.root).unwrap(),
        );
    }
    assert_eq!(staged.request(), prepared.request());
    let receipt = staged.receipt().unwrap();
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
    assert_eq!(
        fs::read_dir(prepared.destination.project().root())
            .unwrap()
            .count(),
        0
    );
    for attempt in &attempts {
        attempt.ensure_namespace_identity().unwrap();
    }
    let editor = prepared
        .destination
        .project()
        .root()
        .join("unexpected-editor-work");
    assert!(prepared
        .stage_with_hook(storage, |step, _| {
            if step == "custody-released" {
                fs::write(&editor, b"retain this editor work")?;
            }
            Ok(())
        })
        .is_err());
    assert_eq!(fs::read(&editor).unwrap(), b"retain this editor work");
    fs::remove_file(&editor).unwrap(); // Test-owned edit, never production recovery cleanup.
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
    let (path, _pin) = staged.root.stable_namespace().unwrap();
    let frames = fs::read(path.join("frames.mesh")).unwrap();
    fs::write(path.join("frames.mesh"), b"changed staged frames").unwrap();
    assert!(prepared.stage(storage).is_err());
    assert_eq!(
        fs::read(path.join("frames.mesh")).unwrap(),
        b"changed staged frames"
    );
    fs::write(path.join("frames.mesh"), frames).unwrap();

    // Even matching rewritten local receipts must not substitute bytes for the signed tree.
    let index = prepared
        .top_entries()
        .iter()
        .position(|(name, _)| name.as_str() == "tree")
        .unwrap();
    let entry = staged
        .root
        .open_child_directory(OsStr::new(&format!("entry-{index}")))
        .unwrap();
    let (entry_path, _entry_pin) = entry.stable_namespace().unwrap();
    let original_file = fs::read(entry_path.join("exchange/run")).unwrap();
    let original_entry = fs::read(entry_path.join(RECEIPT)).unwrap();
    let original_outer = fs::read(path.join(RECEIPT)).unwrap();
    fs::write(entry_path.join("exchange/run"), b"malicious!").unwrap();
    let tree = entry.open_child_directory(OsStr::new("exchange")).unwrap();
    let evidence = crate::managed_file::retained_replacement::observe_tree(
        &tree,
        prepared.limits.entries + 1,
        prepared.limits.bytes,
        prepared.limits.file_bytes,
    )
    .unwrap();
    let mut forged = Json::parse(std::str::from_utf8(&original_entry).unwrap()).unwrap();
    let Json::Object(fields) = &mut forged else {
        panic!()
    };
    fields
        .iter_mut()
        .find(|(key, _)| key == "evidence")
        .unwrap()
        .1 = evidence;
    let forged = forged.encode();
    fs::write(entry_path.join(RECEIPT), forged.as_bytes()).unwrap();
    let mut outer = Json::parse(std::str::from_utf8(&original_outer).unwrap()).unwrap();
    let Json::Object(fields) = &mut outer else {
        panic!()
    };
    let Json::Array(entries) = &mut fields
        .iter_mut()
        .find(|(key, _)| key == "entries")
        .unwrap()
        .1
    else {
        panic!()
    };
    let Json::Object(fields) = &mut entries[index] else {
        panic!()
    };
    fields
        .iter_mut()
        .find(|(key, _)| key == "receipt")
        .unwrap()
        .1 = Json::text(forged);
    fs::write(path.join(RECEIPT), outer.encode()).unwrap();
    assert!(
        prepared.stage(storage).is_err(),
        "rewritten receipts must not authorize unsigned tree content"
    );
    assert_eq!(
        fs::read(entry_path.join("exchange/run")).unwrap(),
        b"malicious!"
    );
    fs::write(entry_path.join("exchange/run"), original_file).unwrap();
    fs::write(entry_path.join(RECEIPT), original_entry).unwrap();
    fs::write(path.join(RECEIPT), original_outer).unwrap();
    assert_eq!(prepared.stage(storage).unwrap().receipt().unwrap(), receipt);
}

#[cfg(test)]
pub(super) fn run_stage_child_if_requested() -> bool {
    let Ok(raw) = std::env::var("MESH_PRIVATE_STAGE_RESTART") else {
        return false;
    };
    use ed25519_dalek::{Signer as _, SigningKey};
    let value = Json::parse(&raw).unwrap();
    let text = |name| value.get(name).and_then(Json::as_text).unwrap();
    let storage = AttachmentStorage::open(Path::new(text("storage"))).unwrap();
    let owner = storage.reopen(text("owner")).unwrap();
    let source = storage.reopen(text("source")).unwrap();
    let destination = storage.reopen(text("destination")).unwrap();
    let version = source
        .saved_versions()
        .unwrap()
        .into_iter()
        .find(|v| v.operation().to_hex() == text("version"))
        .unwrap();
    let key = SigningKey::from_bytes(&[129; 32]);
    // The saved selection is revalidated by native preparation; it is not authority by itself.
    let grant = crate::project_attachment::dependency_transaction::digest(text("grant")).unwrap();
    let candidate = storage
        .prepare_consumed_start(
            &owner,
            NativeConsumedStartRequest {
                input: NativeGrantInspection {
                    source: &source,
                    version,
                    destination: &destination,
                    grant,
                },
                available: &[],
                request: crate::project_attachment::dependency_transaction::digest(text("request"))
                    .unwrap(),
                limits: ObservationLimits::default(),
            },
            PublicKey::from_bytes(key.verifying_key().to_bytes()),
            |payload| {
                Ok::<_, &'static str>(Signature::from_bytes(
                    key.sign(payload.as_bytes()).to_bytes(),
                ))
            },
        )
        .unwrap();
    if text("mode") == "publish" {
        candidate
            .stage_with_hook(&storage, |step, _| {
                if step == "published" {
                    std::process::exit(75);
                }
                Ok(())
            })
            .unwrap();
        panic!("installer should have exited without acknowledgement");
    }
    let staged = candidate.stage(&storage).unwrap();
    assert_eq!(staged.receipt().unwrap().to_hex(), text("receipt"));
    assert_eq!(identity(&staged.root).unwrap(), text("physical"));
    println!("exact private stage recovered");
    true
}

#[cfg(test)]
pub(super) fn assert_revocation_after_staging(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
) {
    let mut revoked = false;
    let mut preserved = None;
    assert!(prepared
        .stage_with_hook(storage, |step, root| {
            if step == "custody-released" {
                // This normal native operation must be able to acquire its complete set after
                // private staging releases custody, and before the final permission recheck.
                storage.grant_saved_input(
                    &prepared.owner,
                    crate::project_attachment::NativeInputGrantRequest {
                        source: &prepared.source,
                        version: prepared.version,
                        destination: &prepared.destination,
                        allowed: false,
                        expected_previous: Some(prepared.grant),
                        request: RecordDigest::from_bytes([91; 32]),
                    },
                )?;
                revoked = true;
                preserved = Some((root.clone(), read(root, RECEIPT, MAX_RECEIPT)?));
            }
            Ok(())
        })
        .is_err());
    assert!(
        revoked,
        "revocation must succeed after private custody is released"
    );
    let (root, receipt) = preserved.unwrap();
    assert_eq!(read(&root, RECEIPT, MAX_RECEIPT).unwrap(), receipt);
    assert!(prepared.stage(storage).is_err());
    assert_eq!(read(&root, RECEIPT, MAX_RECEIPT).unwrap(), receipt);
    assert_eq!(
        fs::read_dir(prepared.destination.project().root())
            .unwrap()
            .count(),
        0
    );
}

#[cfg(test)]
pub(super) fn assert_start_fence(
    prepared: &PreparedNativeConsumedStart,
    storage: &AttachmentStorage,
) {
    start_fence::assert_start_fence(prepared, storage);
}
