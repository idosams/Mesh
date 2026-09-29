//! Bounded native publication for an original received owner; retained offers remain authoritative.
use super::*;
use crate::fleet::service::SavedReviewSelection;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
const MAX_RETAINED: usize = 4096;
const INTERVAL: Duration = Duration::from_secs(1);
#[derive(Default)]
pub(super) struct Publication {
    cursor: Option<String>,
    retained: BTreeMap<String, SavedReviewSelection>,
    next_attempt: Option<Instant>,
}
impl ReceivedWorkerHost {
    /// Offer at most one saved review per second for this original owner. The bounded page cursor
    /// wraps so late completion and newly inserted earlier checkpoint IDs are eventually revisited.
    /// Retained immutable identities avoid repeated exports/signing. Errors preserve all work and
    /// advance the scan so one unavailable result cannot starve later reviews.
    pub fn publish_saved_result(
        &mut self,
        signer: &dyn crate::CheckpointSigner,
    ) -> Option<Result<String, Unavailable>> {
        self.publish_saved_result_at(signer, Instant::now())
    }
    pub(super) fn publish_saved_result_at(
        &mut self,
        signer: &dyn crate::CheckpointSigner,
        now: Instant,
    ) -> Option<Result<String, Unavailable>> {
        if self.publication.next_attempt.is_some_and(|next| now < next) {
            return None;
        }
        self.publication.next_attempt = now.checked_add(INTERVAL);
        match self.publish_next(signer) {
            Ok(Some(checkpoint)) => Some(Ok(checkpoint)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        }
    }
    fn publish_next(
        &mut self,
        signer: &dyn crate::CheckpointSigner,
    ) -> Result<Option<String>, Unavailable> {
        let work = self.receipt.admission().work();
        if mesh_store::RecordDigest::from_bytes(*signer.public_key().as_bytes()).to_string()
            != work.assignment.worker_key
        {
            return Err(unavailable("remote-result-worker-key-mismatch"));
        }
        let page = self
            .service
            .saved_reviews(&work.lane, self.publication.cursor.as_deref())?;
        let Some(Json::Array(rows)) = page.get("reviews") else {
            return Err(unavailable("remote-result-page-unavailable"));
        };
        for row in rows {
            let text = |key| {
                row.get(key)
                    .and_then(Json::as_text)
                    .ok_or_else(|| unavailable("remote-result-page-unavailable"))
            };
            let checkpoint = text("checkpoint")?;
            self.publication.cursor = Some(checkpoint.to_owned());
            let selection = SavedReviewSelection::new(
                &work.lane,
                checkpoint,
                text("version")?,
                text("bundle")?,
            )?;
            if text("run")? != work.run {
                return Err(unavailable("remote-result-attempt-mismatch"));
            }
            if let Some(retained) = self.publication.retained.get(checkpoint) {
                if retained != &selection {
                    return Err(unavailable("remote-result-selection-changed"));
                }
                continue;
            }
            if self.publication.retained.len() >= MAX_RETAINED {
                return Err(unavailable("remote-result-publication-limit"));
            }
            let (offer, source) = self
                .service
                .sign_remote_saved_review(&selection, |payload| signer.sign(payload))?;
            // The durable offer precedes this cache entry. Dropping these handles does not claim
            // transport completion or delete content; later transfer must reopen exact saved data.
            drop((offer, source));
            self.publication
                .retained
                .insert(checkpoint.to_owned(), selection);
            return Ok(Some(checkpoint.to_owned()));
        }
        self.publication.cursor = match page.get("next_after") {
            Some(Json::Null) => None,
            Some(value) => Some(
                value
                    .as_text()
                    .ok_or_else(|| unavailable("remote-result-page-unavailable"))?
                    .to_owned(),
            ),
            None => return Err(unavailable("remote-result-page-unavailable")),
        };
        Ok(None)
    }
}
