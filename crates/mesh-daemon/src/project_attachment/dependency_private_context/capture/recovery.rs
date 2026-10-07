//! Exact capture-prefix evidence; never ordinary workspace or publication admission.
use super::*;
use crate::project_attachment::history::dependency_capture::{capture_prefix, validate_frames};

pub(in crate::project_attachment) struct VerifiedCapturePrefix {
    work: ProvisionedAttachment,
    pub(super) raw: String,
    pub(super) intent: CaptureIntent,
    pub(super) pending: bool,
    observed: RecordDigest,
    store: (u64, u64),
    pub(super) frames: Vec<u8>,
}
impl VerifiedCapturePrefix {
    pub(super) fn read(work: &ProvisionedAttachment, request: RecordDigest) -> io::Result<Self> {
        if request == RecordDigest::from_bytes([0; 32]) {
            return Err(invalid("missing capture recovery request"));
        }
        let (raw, pending) = match read_private_in_store(&work.store, PENDING) {
            Ok(raw) => (raw, true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (
                read_private_in_store(&work.store, &receipt_name(request))?,
                false,
            ),
            Err(e) => return Err(e),
        };
        let intent = CaptureIntent::parse(&raw)?;
        if intent.request != request {
            return Err(invalid("another capture request owns recovery"));
        }
        let mut journal = work
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut journal)?;
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            work.metadata_path(),
            work.store.filesystem().read_only(),
        )
        .map_err(error)?;
        let frames = validate_frames(&cas, &intent)?;
        let metadata = journal.metadata()?;
        if (metadata.dev(), metadata.ino()) != intent.journal
            || intent.before_bytes > bytes.len()
            || intent.before_bytes.saturating_add(frames.len()) > MAX_JOURNAL
            || hash(&bytes[..intent.before_bytes]) != intent.before_digest
        {
            return Err(invalid("capture recovery prefix changed"));
        }
        let suffix = &bytes[intent.before_bytes..];
        if pending && suffix.len() > frames.len() {
            return Err(invalid("foreign bytes follow pending capture"));
        }
        if suffix.len() < frames.len() {
            if !pending || !frames.starts_with(suffix) {
                return Err(invalid("foreign or incomplete capture receipt"));
            }
        } else if !suffix.starts_with(&frames) {
            return Err(invalid("capture receipt names different committed bytes"));
        }
        let result = Self {
            work: work.clone(),
            store: work.store.identity()?,
            raw,
            intent,
            pending,
            observed: hash(&bytes),
            frames,
        };
        result.verify_observed(&work.store, &journal, &bytes)?;
        Ok(result)
    }
    pub(in crate::project_attachment) fn applies(&self, work: &ProvisionedAttachment) -> bool {
        self.work.id() == work.id()
    }
    fn verify_observed(
        &self,
        store: &crate::root_authority::PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
    ) -> io::Result<()> {
        store.ensure_namespace_identity()?;
        let metadata = file.metadata()?;
        let named = store
            .filesystem()
            .inspect_entry(Path::new(crate::RECORD_FILE_NAME))?
            .metadata()?;
        let receipt = receipt_name(self.intent.request);
        if store.identity()? != self.store
            || !metadata.is_file()
            || metadata.nlink() != 1
            || (metadata.dev(), metadata.ino()) != self.intent.journal
            || (named.dev(), named.ino()) != self.intent.journal
            || hash(bytes) != self.observed
            || read_private_in_store(store, if self.pending { PENDING } else { &receipt })?
                != self.raw
        {
            return Err(invalid("capture recovery evidence changed"));
        }
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.work.metadata_path(),
            store.filesystem().read_only(),
        )
        .map_err(error)?;
        if validate_frames(&cas, &self.intent)? != self.frames {
            return Err(invalid("capture recovery frames changed"));
        }
        Ok(())
    }
    pub(in crate::project_attachment) fn verify(
        &self,
        store: &crate::root_authority::PinnedWorkspaceRoot,
        file: &File,
        bytes: &[u8],
        authority: RecordDigest,
        configuration: &str,
    ) -> io::Result<usize> {
        self.verify_observed(store, file, bytes)?;
        let cas = Cas::<PinnedRootFs, Blake3>::with_filesystem(
            self.work.metadata_path(),
            store.filesystem().read_only(),
        )
        .map_err(error)?;
        capture_prefix(
            &cas,
            &self.raw,
            self.intent.journal,
            bytes,
            authority,
            configuration,
        )
    }
    pub(in crate::project_attachment) fn raw_for<'a>(
        &'a self,
        work: &ProvisionedAttachment,
    ) -> io::Result<&'a str> {
        if !self.applies(work) {
            return Err(invalid("capture recovery belongs to another work"));
        }
        let mut file = work
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        self.verify_observed(&work.store, &file, &bytes)?;
        Ok(&self.raw)
    }
    pub(in crate::project_attachment) fn checked_raw<'a>(
        &'a self,
        work: &ProvisionedAttachment,
        configuration: &str,
    ) -> io::Result<&'a str> {
        if !self.applies(work) {
            return Err(invalid("capture recovery belongs to another work"));
        }
        let mut file = work
            .store
            .filesystem()
            .read_only()
            .read_file(Path::new(crate::RECORD_FILE_NAME))?;
        let bytes = journal_bytes(&mut file)?;
        self.verify(
            &work.store,
            &file,
            &bytes,
            self.intent.authority,
            configuration,
        )?;
        Ok(&self.raw)
    }
}
