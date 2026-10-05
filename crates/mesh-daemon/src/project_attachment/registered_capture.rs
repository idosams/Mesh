//! Registration-aware capture attempts for native schedulers.
use super::*;
use crate::project_attachment::AttachmentStorage;

impl AttachmentStorage {
    /// Save one complete observation using the current registered history format. Interrupted
    /// native saves recover their exact retained request before considering newer input. This
    /// neither enrolls ordinary projects nor grants execution or protected-main authority.
    pub fn save_registered_capture<F, E>(
        &self,
        selected: &ProvisionedAttachment,
        input: &CapturedProjectInput,
        actor: PublicKey,
        sign: F,
    ) -> io::Result<SavedAttachmentVersion>
    where
        F: FnOnce(&SigningPayload) -> Result<Signature, E>,
        E: std::fmt::Display,
    {
        let current = self.exact_registered_work(selected)?;
        let pending = {
            let _guard = crate::workspace_custody::lock_workspace_initialization(&current.store)
                .map_err(error)?;
            match read_private_in_store(&current.store, PENDING) {
                Ok(raw) => Some(CaptureIntent::parse(&raw)?.request),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            }
        };
        if let Some(request) = pending {
            self.recover_registered_dependency_capture(&current, request)?;
        }
        let native = {
            let _guard = crate::workspace_custody::lock_workspace_initialization(&current.store)
                .map_err(error)?;
            match read_private_in_store(&current.store, crate::project_attachment::history::HISTORY)
            {
                Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                Err(e) => return Err(e),
                Ok(_) => current
                    .project()
                    .read_native_facts(current.metadata_path(), &current.store, None, None)?
                    .1
                    .is_some(),
            }
        };
        if !native {
            return current.project().save_capture_in_store(
                current.metadata_path(),
                input,
                actor,
                sign,
                current.store.clone(),
            );
        }
        let unchanged = self.with_registered_dependency_history(
            current.id(),
            |work, history, configuration| {
                self.exact_registered_work(&current)?;
                work.project().ensure_current()?;
                crate::project_attachment::detachment::ensure_attached(&work.store)?;
                if input.root() != work.project().root()
                    || input.identity() != work.project().pinned.identity()?
                {
                    return Err(invalid("capture belongs to a different native attachment"));
                }
                work.project().history_configuration_with_previous(
                    &work.store,
                    Some(input.exclusion_digest()),
                    Some(configuration.to_owned()),
                )?;
                let line = CaptureLine::load(&work.store, history, configuration)?;
                let unchanged = if let Some(head) = line.head {
                    let basis = history
                        .historical_authoring_basis(head, actor)
                        .map_err(error)?;
                    let (operations, _) = prepare_snapshot(history, input, &basis, Some(head))?;
                    if operations.is_empty() {
                        Some(SavedAttachmentVersion::from_verified_history(
                            history, head,
                        )?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.exact_registered_work(&current)?;
                Ok(unchanged)
            },
        )?;
        if let Some(saved) = unchanged {
            return Ok(saved);
        }
        // The durable writer records this nonce before any append. Restart recovery reads that
        // exact intent rather than inventing a replacement request for an ambiguous commit.
        let mut nonce = [0; 32];
        fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        self.prepare_registered_dependency_capture(
            &current,
            input,
            actor,
            RecordDigest::from_bytes(nonce),
            sign,
        )?
        .commit()
    }
}
