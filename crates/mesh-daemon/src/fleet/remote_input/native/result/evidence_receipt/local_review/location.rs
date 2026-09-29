//! Read retained native history by immutable identities alone, without peer or renderer paths.
use super::*;
fn stream(receipt: &RemoteLocalReviewReceipt) -> String {
    format!("result-review-location-{}", receipt.digest())
}
impl Runtime {
    fn remote_review_location(
        &self,
        receipt: &RemoteLocalReviewReceipt,
    ) -> Result<Option<String>, Error> {
        let events = self.store.events(&stream(receipt), 0, 2)?;
        match events.as_slice() {
            [] => Ok(None),
            [event]
                if event.revision == 1
                    && event.request == "location"
                    && event.payload.len() <= 16_384 =>
            {
                Ok(Some(event.payload.clone()))
            }
            _ => Err(refused()),
        }
    }
    pub(super) fn record_remote_review_location(
        &mut self,
        receipt: &RemoteLocalReviewReceipt,
        destination: &RemoteInputDestination,
    ) -> Result<(), Error> {
        if receipt.objective != self.objective() {
            return Err(refused());
        }
        let encoded = destination.history_binding().map_err(store_error)?.encode();
        if let Some(previous) = self.remote_review_location(receipt)? {
            return if previous == encoded {
                Ok(())
            } else {
                Err(refused())
            };
        }
        let result = self
            .store
            .append_with_outcome(&stream(receipt), 0, "location", &encoded);
        match self.remote_review_location(receipt)? {
            Some(previous) if previous == encoded => Ok(()),
            Some(_) => Err(refused()),
            None => {
                result?;
                Err(refused())
            }
        }
    }
    fn with_remote_review<T>(
        &self,
        offer: RecordDigest,
        correlation: RecordDigest,
        read: impl FnOnce(
            &RemoteLocalReviewReceipt,
            &RemoteInputDestination,
            &RemoteInputManifest,
        ) -> io::Result<T>,
    ) -> Result<T, Error> {
        let receipt = self
            .retained_remote_local_review(offer)?
            .filter(|r| r.digest() == correlation)
            .ok_or_else(refused)?;
        let raw = self.remote_review_location(&receipt)?.ok_or_else(refused)?;
        let value = Json::parse(&raw).map_err(|_| refused())?;
        if value.encode() != raw {
            return Err(refused());
        }
        let destination =
            RemoteInputDestination::from_history_binding(&value).map_err(store_error)?;
        let (root, _) = destination.receiving_store().map_err(store_error)?;
        let path = PathBuf::from(format!("result-manifest-{}.json", receipt.remote_manifest));
        let bytes = super::super::super::receipt::read_metadata(&root, &path, 1_048_576)
            .map_err(store_error)?
            .ok_or_else(refused)?;
        let encoded = std::str::from_utf8(&bytes).map_err(|_| refused())?;
        let manifest =
            RemoteInputManifest::decode(encoded, receipt.remote_version, receipt.remote_manifest)?;
        let result = read(&receipt, &destination, &manifest).map_err(store_error)?;
        if self.retained_remote_local_review(offer)?.as_ref() != Some(&receipt)
            || self.remote_review_location(&receipt)?.as_deref() != Some(&raw)
            || destination.history_binding().map_err(store_error)? != value
        {
            return Err(refused());
        }
        Ok(result)
    }
    /// Reopen an exact review from the native retained location. No path crosses this API.
    /// Legacy unbound records remain discoverable but unavailable; reads never bind or repair them.
    pub fn remote_saved_review(
        &self,
        offer: RecordDigest,
        correlation: RecordDigest,
        reviewers: &crate::TrustedReviewers,
    ) -> Result<Json, Error> {
        self.with_remote_review(offer, correlation, |receipt, destination, manifest| {
            receipt.saved_review(destination, manifest, reviewers)
        })
    }
    /// Read an exact retained artifact without an active attempt or receiving-store mutation.
    pub fn remote_saved_review_artifact(
        &self,
        offer: RecordDigest,
        correlation: RecordDigest,
        reviewers: &crate::TrustedReviewers,
        selection: (&str, &str),
    ) -> Result<crate::ReviewArtifact, Error> {
        self.with_remote_review(offer, correlation, |receipt, destination, manifest| {
            receipt.saved_review_artifact(destination, manifest, reviewers, selection)
        })
    }
}
