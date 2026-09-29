//! Coordinator receipt of signed immutable results, separate from remote input admission.
use super::*;
use crate::fleet::{RemoteSavedResultOffer, RemoteWorkerStatusRequest, Runtime};
use mesh_types::PublicKey;

/// Native private-store receiver bound to one exact signed result and coordinator assignment.
/// Partial CAS offsets survive dropping this value. Reopening requires renewed native validation;
/// no execution/input reservation is fabricated and no working folder is materialized.
pub struct NativeRemoteResultReceiver<'a> {
    manifest: RemoteInputManifest,
    offer: String,
    lane: String,
    run: String,
    coordinator: PublicKey,
    worker: PublicKey,
    cas: Cas<ReceivingFs<'a>>,
    destination: &'a RemoteInputDestination,
    _lock: File,
}
impl<'a> NativeRemoteResultReceiver<'a> {
    /// Verify the worker signature, current assignment and full manifest before any store effect.
    /// The destination is independently admitted by native policy, never selected by peer bytes.
    pub fn new(
        destination: &'a RemoteInputDestination,
        manifest: RemoteInputManifest,
        encoded_offer: &str,
        request: RemoteWorkerStatusRequest<'_>,
    ) -> Result<Self, Error> {
        RemoteSavedResultOffer::verify(
            request.runtime,
            request.lane,
            request.run,
            request.coordinator,
            request.worker,
            encoded_offer,
            &manifest,
        )?;
        let (root, path) = destination.receiving_store().map_err(store_error)?;
        let lock = lock(&root).map_err(|_| Error::Refused("remote-result-store-busy"))?;
        destination.verify().map_err(store_error)?;
        let cas = Cas::with_filesystem(path, ReceivingFs { root, destination })
            .map_err(|_| Error::Refused("remote-result-store"))?;
        let receiver = Self {
            manifest,
            offer: encoded_offer.to_owned(),
            lane: request.lane.to_owned(),
            run: request.run.to_owned(),
            coordinator: request.coordinator,
            worker: request.worker,
            cas,
            destination,
            _lock: lock,
        };
        receiver.check(request.runtime)?;
        Ok(receiver)
    }
    fn check(&self, runtime: &mut Runtime) -> Result<(), Error> {
        self.destination.verify().map_err(store_error)?;
        RemoteSavedResultOffer::verify(
            runtime,
            &self.lane,
            &self.run,
            self.coordinator,
            self.worker,
            &self.offer,
            &self.manifest,
        )?;
        self.destination.verify().map_err(store_error)
    }
    fn receiver(&self) -> RemoteInputReceiver<'_, ReceivingFs<'a>> {
        RemoteInputReceiver {
            manifest: Cow::Borrowed(&self.manifest),
            cas: &self.cas,
        }
    }
    /// Exact immutable manifest already verified against the original signed offer.
    pub fn manifest(&self) -> &RemoteInputManifest {
        &self.manifest
    }
    /// Recover one declared chunk's durable offset and verified-complete flag.
    pub fn status(&self, runtime: &mut Runtime, digest: Digest32) -> Result<(u64, bool), Error> {
        self.check(runtime)?;
        let status = self.receiver().status(digest)?;
        self.check(runtime)?;
        Ok(status)
    }
    /// Retain a bounded contiguous part. Successful return follows durable receipt and native
    /// context/root rechecks. Error preserves uncertainty; it is not permission to restart work.
    pub fn accept(
        &mut self,
        runtime: &mut Runtime,
        digest: Digest32,
        offset: u64,
        bytes: &[u8],
        final_part: bool,
    ) -> Result<(), Error> {
        self.check(runtime)?;
        self.receiver().accept(digest, offset, bytes, final_part)?;
        self.check(runtime)
    }
    /// Verify every complete file hash through pinned storage. Success is a current content check,
    /// not a durable completion/import receipt, retention pin, process result or main approval.
    pub fn verify_complete(&self, runtime: &mut Runtime) -> Result<(), Error> {
        self.check(runtime)?;
        self.receiver().verify_complete()?;
        self.check(runtime)
    }
}

mod receipt;
pub use receipt::RemoteResultContentReceipt;

mod evidence_receipt;
pub use evidence_receipt::{RemoteLocalReviewReceipt, RemoteResultEvidenceReceipt};
