//! Durable coordinator content observation, distinct from execution completion and candidate import.
use super::*;
use std::os::unix::fs::PermissionsExt as _;
const SCHEMA: &str = "mesh.remote-result-content-receipt/v1";

/// Exact durable content receipt. Availability must be reverified after restart; this is not a
/// retention pin, process-completion fact, candidate import or approval capability.
pub struct RemoteResultContentReceipt {
    digest: RecordDigest,
}
impl RemoteResultContentReceipt {
    /// Domain-bound identity of the exact offer, immutable manifest and native receiving store.
    pub fn digest(&self) -> RecordDigest {
        self.digest
    }
}
fn refused() -> Error {
    Error::Refused("remote-result-content-receipt")
}
fn manifest_path(bundle: RecordDigest) -> PathBuf {
    PathBuf::from(format!("result-manifest-{bundle}.json"))
}
fn read_manifest(root: &PinnedWorkspaceRoot, path: &Path) -> io::Result<Option<Vec<u8>>> {
    read_metadata(root, path, MAX_MANIFEST_BYTES)
}
pub(super) fn read_metadata(
    root: &PinnedWorkspaceRoot,
    path: &Path,
    maximum: usize,
) -> io::Result<Option<Vec<u8>>> {
    let file = match root.filesystem().read_only().read_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if metadata.nlink() != 1
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > maximum as u64
    {
        return Err(io::Error::other(
            "result manifest is not a bounded private regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(io::Error::other("result manifest grew"));
    }
    Ok(Some(bytes))
}
impl<'a> NativeRemoteResultReceiver<'a> {
    fn receipt_body(&self) -> Result<Json, Error> {
        self.destination.verify().map_err(store_error)?;
        let (root, _) = self.destination.receiving_store().map_err(store_error)?;
        let (device, inode) = root.identity().map_err(store_error)?;
        Ok(Json::object([
            ("schema", Json::text(SCHEMA)),
            ("offer", Json::text(&self.offer)),
            ("manifest", Json::text(self.manifest.bundle().to_string())),
            ("store", Json::text(format!("{device:016x}:{inode:016x}"))),
        ]))
    }
    fn retained_receipt(
        &self,
        runtime: &Runtime,
        body: &Json,
    ) -> Result<Option<RemoteResultContentReceipt>, Error> {
        let digest =
            RecordDigest::from_bytes(*Blake3::digest_bytes(body.encode().as_bytes()).as_bytes());
        let stream = format!("result-content-{digest}");
        let events = runtime.store.events(&stream, 0, 2)?;
        match events.as_slice() {
            [] => Ok(None),
            [event]
                if event.revision == 1
                    && event.request == "content"
                    && event.payload == body.encode() =>
            {
                Ok(Some(RemoteResultContentReceipt { digest }))
            }
            _ => Err(refused()),
        }
    }
    /// Verify all content, durably retain its manifest, then record one exact coordinator receipt.
    /// Repeating after a lost reply returns the same identity. Conflicting/partial metadata refuses
    /// and is preserved. This never changes the objective revision, run status or concurrency slots.
    pub fn record_content_receipt(
        &self,
        runtime: &mut Runtime,
    ) -> Result<RemoteResultContentReceipt, Error> {
        self.verify_complete(runtime)?;
        let body = self.receipt_body()?;
        let (root, _) = self.destination.receiving_store().map_err(store_error)?;
        let path = manifest_path(self.manifest.bundle());
        let retained = self.retained_receipt(runtime, &body)?;
        if read_manifest(&root, &path).map_err(store_error)?.is_none() {
            if retained.is_some() {
                return Err(refused());
            }
            match root.filesystem().write_new_file(
                &path,
                self.manifest.encoded().as_bytes(),
                std::fs::Permissions::from_mode(0o600),
            ) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(store_error(error)),
            }
        }
        if read_manifest(&root, &path).map_err(store_error)?.as_deref()
            != Some(self.manifest.encoded().as_bytes())
        {
            return Err(refused());
        }
        // Retry also flushes a valid file retained by an interrupted earlier publication.
        root.filesystem()
            .read_only()
            .read_file(&path)
            .map_err(store_error)?
            .sync_all()
            .map_err(store_error)?;
        root.sync().map_err(store_error)?;
        self.verify_complete(runtime)?;
        if self.receipt_body()? != body {
            return Err(refused());
        }
        if let Some(receipt) = self.retained_receipt(runtime, &body)? {
            if read_manifest(&root, &path).map_err(store_error)?.as_deref()
                != Some(self.manifest.encoded().as_bytes())
            {
                return Err(refused());
            }
            self.check(runtime)?;
            return Ok(receipt);
        }
        let digest =
            RecordDigest::from_bytes(*Blake3::digest_bytes(body.encode().as_bytes()).as_bytes());
        let result = runtime.store.append_with_outcome(
            &format!("result-content-{digest}"),
            0,
            "content",
            &body.encode(),
        );
        let receipt = match self.retained_receipt(runtime, &body)? {
            Some(receipt) => receipt,
            None => {
                result?;
                return Err(refused());
            }
        };
        self.verify_complete(runtime)?;
        if self.receipt_body()? != body {
            return Err(refused());
        }
        if read_manifest(&root, &path).map_err(store_error)?.as_deref()
            != Some(self.manifest.encoded().as_bytes())
        {
            return Err(refused());
        }
        self.check(runtime)?;
        Ok(receipt)
    }
    /// Reopen only an already recorded receipt using the exact native destination and current
    /// assignment. Missing manifest, ledger fact or content refuses; nothing is repaired or imported.
    pub fn reopen_content_receipt(
        destination: &'a RemoteInputDestination,
        encoded_offer: &str,
        request: RemoteWorkerStatusRequest<'_>,
    ) -> Result<(Self, RemoteResultContentReceipt), Error> {
        let (version, bundle) = RemoteSavedResultOffer::content_identity(encoded_offer)?;
        let (root, _) = destination.receiving_store().map_err(store_error)?;
        let bytes = read_manifest(&root, &manifest_path(bundle))
            .map_err(store_error)?
            .ok_or_else(refused)?;
        let manifest = RemoteInputManifest::decode(
            std::str::from_utf8(&bytes).map_err(|_| refused())?,
            version,
            bundle,
        )?;
        let runtime = request.runtime;
        let receiver = Self::new(
            destination,
            manifest,
            encoded_offer,
            RemoteWorkerStatusRequest {
                runtime,
                lane: request.lane,
                run: request.run,
                coordinator: request.coordinator,
                worker: request.worker,
            },
        )?;
        receiver.verify_complete(runtime)?;
        let body = receiver.receipt_body()?;
        let receipt = receiver
            .retained_receipt(runtime, &body)?
            .ok_or_else(refused)?;
        if read_manifest(&root, &manifest_path(bundle)).map_err(store_error)? != Some(bytes) {
            return Err(refused());
        }
        receiver.check(runtime)?;
        Ok((receiver, receipt))
    }
}
