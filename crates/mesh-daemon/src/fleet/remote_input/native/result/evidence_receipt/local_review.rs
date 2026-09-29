//! Durable remote/local review correlation. Historical reading grants no run or import authority.
use super::*;
use crate::fleet::{ReceivedResultWorkspace, RemoteInputSource};
use mesh_types::PublicKey;

/// Native-verified remote/local review identity retained in the coordinator ledger.
/// Fields cannot be constructed by a peer; reopening still verifies exact local custody/history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteLocalReviewReceipt {
    offer: RecordDigest,
    evidence: RecordDigest,
    content: RecordDigest,
    mapping: RecordDigest,
    version: RecordDigest,
    review: RecordDigest,
    remote_version: RecordDigest,
    remote_manifest: RecordDigest,
    allocation: String,
    lane: String,
    run: String,
    objective: String,
}
impl RemoteLocalReviewReceipt {
    fn encoded(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh.remote-local-review/v1")),
            ("offer", Json::text(self.offer.to_string())),
            ("evidence", Json::text(self.evidence.to_string())),
            ("content", Json::text(self.content.to_string())),
            ("mapping", Json::text(self.mapping.to_string())),
            ("version", Json::text(self.version.to_string())),
            ("review", Json::text(self.review.to_string())),
            (
                "remote_version",
                Json::text(self.remote_version.to_string()),
            ),
            (
                "remote_manifest",
                Json::text(self.remote_manifest.to_string()),
            ),
            ("allocation", Json::text(&self.allocation)),
            ("lane", Json::text(&self.lane)),
            ("run", Json::text(&self.run)),
            ("objective", Json::text(&self.objective)),
        ])
        .encode()
    }
    fn decode(raw: &str) -> Result<Self, Error> {
        if raw.len() > 16_384 {
            return Err(refused());
        }
        let value = Json::parse(raw).map_err(|_| refused())?;
        let digest = |name| RecordDigest::parse_hex(text(&value, name)?).map_err(|_| refused());
        let result = Self {
            offer: digest("offer")?,
            evidence: digest("evidence")?,
            content: digest("content")?,
            mapping: digest("mapping")?,
            version: digest("version")?,
            review: digest("review")?,
            remote_version: digest("remote_version")?,
            remote_manifest: digest("remote_manifest")?,
            allocation: text(&value, "allocation")?.to_owned(),
            lane: text(&value, "lane")?.to_owned(),
            run: text(&value, "run")?.to_owned(),
            objective: text(&value, "objective")?.to_owned(),
        };
        if result.encoded() != raw
            || result.allocation.len() != 32
            || !result
                .allocation
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(refused());
        }
        for id in [&result.lane, &result.run, &result.objective] {
            crate::fleet::id_valid(id)?;
        }
        Ok(result)
    }
    /// Stable correlation identity, distinct from an execution or main-approval receipt.
    pub fn digest(&self) -> RecordDigest {
        hash(&self.encoded())
    }
    /// Exact local saved review bundle.
    pub fn review(&self) -> RecordDigest {
        self.review
    }
    /// Exact local subject operation, distinct from the remote worker's operation.
    pub fn version(&self) -> RecordDigest {
        self.version
    }
    /// Reopen historical review content independently of current run state. Native storage and
    /// the recorded review/subject must still match; missing state is never repaired.
    pub fn reopen(
        &self,
        destination: &RemoteInputDestination,
        manifest: &RemoteInputManifest,
        reviewers: &crate::TrustedReviewers,
    ) -> io::Result<RemoteInputSource> {
        if manifest.input() != self.remote_version || manifest.bundle() != self.remote_manifest {
            return Err(io::Error::other("remote result manifest changed"));
        }
        destination.reopen_result_history_checked(
            &self.allocation,
            self.mapping,
            self.evidence,
            manifest,
            reviewers,
            Some((self.version, self.review)),
        )
    }
}
impl Runtime {
    /// Recover a previously verified correlation without adopting a run or asserting availability.
    /// Call receipt.reopen before displaying saved content. This is not import eligibility.
    pub fn retained_remote_local_review(
        &self,
        offer: RecordDigest,
    ) -> Result<Option<RemoteLocalReviewReceipt>, Error> {
        let events = self
            .store
            .events(&format!("result-local-review-{offer}"), 0, 2)?;
        let event = match events.as_slice() {
            [] => return Ok(None),
            [event] if event.revision == 1 && event.request == "review" => event,
            _ => return Err(refused()),
        };
        let receipt = RemoteLocalReviewReceipt::decode(&event.payload)?;
        if receipt.offer != offer || receipt.objective != self.objective() {
            return Err(refused());
        }
        Ok(Some(receipt))
    }
}
impl NativeRemoteResultReceiver<'_> {
    /// Correlate an authenticated durable result with its exact native local review. Existing
    /// correlation is immutable; another allocation for the same offer cannot replace it.
    pub fn record_local_review(
        &self,
        runtime: &mut Runtime,
        input: &RemoteInputManifest,
        evidence: &RemoteResultEvidenceReceipt,
        local: &ReceivedResultWorkspace,
        actor: PublicKey,
    ) -> Result<RemoteLocalReviewReceipt, Error> {
        let content = self.verify_content_receipt(runtime)?.digest();
        let retained = self
            .load_evidence(runtime, input, content)?
            .ok_or_else(refused)?;
        local.verify().map_err(store_error)?;
        if retained.digest() != evidence.digest()
            || local.evidence_receipt() != retained.digest()
            || local.binding().source_version != self.manifest.input()
        {
            return Err(refused());
        }
        let offer = hash(&self.offer);
        let previous = runtime.retained_remote_local_review(offer)?;
        if previous
            .as_ref()
            .is_some_and(|old| old.mapping != local.receipt_digest())
        {
            return Err(refused());
        }
        let review = local.record_review(actor).map_err(store_error)?;
        let receipt = RemoteLocalReviewReceipt {
            offer,
            evidence: retained.digest(),
            content,
            mapping: local.receipt_digest(),
            version: local.binding().starting_version.ok_or_else(refused)?,
            review,
            remote_version: self.manifest.input(),
            remote_manifest: self.manifest.bundle(),
            allocation: local.allocation_id().map_err(store_error)?,
            lane: self.lane.clone(),
            run: self.run.clone(),
            objective: runtime.objective().to_owned(),
        };
        if previous.as_ref().is_some_and(|old| old != &receipt) {
            return Err(refused());
        }
        runtime.index_remote_local_review(&receipt)?;
        if previous.is_none() {
            let result = runtime.store.append_with_outcome(
                &format!("result-local-review-{offer}"),
                0,
                "review",
                &receipt.encoded(),
            );
            match runtime.retained_remote_local_review(offer)? {
                Some(old) if old == receipt => (),
                Some(_) => return Err(refused()),
                None => {
                    result?;
                    return Err(refused());
                }
            }
        }
        self.verify_complete(runtime)?;
        local.verify().map_err(store_error)?;
        Ok(receipt)
    }
}

mod catalog;
pub use catalog::{RemoteLocalReviewEntry, RemoteLocalReviewPage};
