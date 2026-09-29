//! Bounded offline discovery. Index intent precedes correlation commitment; gaps stay explicit.
use super::*;
const LIMIT: u64 = 4096;
const PAGE: usize = 16;
fn stream(runtime: &Runtime) -> String {
    format!("result-local-index-{}", hash(runtime.objective()))
}
fn payload(receipt: &RemoteLocalReviewReceipt) -> String {
    Json::object([
        ("schema", Json::text("mesh.remote-local-review-index/v1")),
        ("receipt", Json::text(receipt.encoded())),
    ])
    .encode()
}
/// One durable discovery intent. A missing receipt is an interrupted correlation, never readiness.
pub struct RemoteLocalReviewEntry {
    /// Stable append sequence in this objective's bounded discovery index.
    pub sequence: u64,
    /// Original signed offer identity; never a peer pathname.
    pub offer: RecordDigest,
    /// Complete matching correlation, if committed. Reopen separately to verify local availability.
    pub receipt: Option<RemoteLocalReviewReceipt>,
}
/// A bounded snapshot of offline remote review discovery; later appends do not move this snapshot.
pub struct RemoteLocalReviewPage {
    /// Fixed index revision to pass into subsequent page reads.
    pub snapshot: u64,
    /// Next exclusive sequence cursor, absent when this snapshot is exhausted.
    pub next: Option<u64>,
    /// At most sixteen entries, including explicit interrupted correlation intents.
    pub entries: Vec<RemoteLocalReviewEntry>,
}
impl Runtime {
    pub(super) fn index_remote_local_review(
        &mut self,
        receipt: &RemoteLocalReviewReceipt,
    ) -> Result<(), Error> {
        if receipt.objective != self.objective() {
            return Err(refused());
        }
        let stream = stream(self);
        let request = receipt.offer.to_string();
        let encoded = payload(receipt);
        let revision = self.store.revision(&stream)?;
        if revision > LIMIT {
            return Err(refused());
        }
        if let Some(event) = self.store.request(&stream, &request)? {
            return if event.revision <= LIMIT && event.payload == encoded {
                Ok(())
            } else {
                Err(refused())
            };
        }
        if revision >= LIMIT {
            return Err(refused());
        }
        let result = self
            .store
            .append_with_outcome(&stream, revision, &request, &encoded);
        match self.store.request(&stream, &request)? {
            Some(event) if event.revision <= LIMIT && event.payload == encoded => Ok(()),
            Some(_) => Err(refused()),
            None => {
                result?;
                Err(refused())
            }
        }
    }
    /// Discover retained remote reviews offline without needing a worker or active run. Index
    /// intent precedes per-offer commitment, so interruption is shown as missing correlation.
    /// Existing v1 correlations without an index remain readable by offer; exact native
    /// re-registration can index them. This read never repairs or imports history.
    pub fn remote_local_reviews(
        &self,
        after: u64,
        snapshot: Option<u64>,
    ) -> Result<RemoteLocalReviewPage, Error> {
        let stream = stream(self);
        let current = self.store.revision(&stream)?;
        let snapshot = snapshot.unwrap_or(current);
        if current > LIMIT || snapshot > current || after > snapshot {
            return Err(refused());
        }
        let expected = (snapshot - after).min(PAGE as u64) as usize;
        if expected == 0 {
            return Ok(RemoteLocalReviewPage {
                snapshot,
                next: None,
                entries: vec![],
            });
        }
        let events = self.store.events(&stream, after, expected)?;
        if events.len() != expected {
            return Err(refused());
        }
        let mut entries = Vec::with_capacity(expected);
        for (index, event) in events.iter().enumerate() {
            if event.revision != after + index as u64 + 1 {
                return Err(refused());
            }
            let value = Json::parse(&event.payload).map_err(|_| refused())?;
            let receipt = RemoteLocalReviewReceipt::decode(text(&value, "receipt")?)?;
            if receipt.objective != self.objective()
                || event.request != receipt.offer.to_string()
                || event.payload != payload(&receipt)
            {
                return Err(refused());
            }
            let committed = self.retained_remote_local_review(receipt.offer)?;
            if committed.as_ref().is_some_and(|value| value != &receipt) {
                return Err(refused());
            }
            entries.push(RemoteLocalReviewEntry {
                sequence: event.revision,
                offer: receipt.offer,
                receipt: committed,
            });
        }
        let end = after + expected as u64;
        Ok(RemoteLocalReviewPage {
            snapshot,
            next: (end < snapshot).then_some(end),
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::remote_admission::authentication::tests::Fixture;
    fn receipt(n: u8) -> RemoteLocalReviewReceipt {
        let d = RecordDigest::from_bytes([n; 32]);
        RemoteLocalReviewReceipt {
            offer: d,
            evidence: d,
            content: d,
            mapping: d,
            version: d,
            review: d,
            remote_version: d,
            remote_manifest: d,
            allocation: "01".repeat(16),
            lane: "lane".into(),
            run: "run".into(),
            objective: "objective".into(),
        }
    }
    #[test]
    fn offline_review_pages_pin_snapshot_and_preserve_interrupted_intents() {
        let f = Fixture::new();
        let mut runtime = f.runtime(true);
        assert!(runtime
            .remote_local_reviews(0, None)
            .unwrap()
            .entries
            .is_empty());
        for n in 1..=18 {
            runtime.index_remote_local_review(&receipt(n)).unwrap();
        }
        let page = runtime.remote_local_reviews(0, None).unwrap();
        assert_eq!(
            (page.snapshot, page.next, page.entries.len()),
            (18, Some(16), 16)
        );
        assert!(page.entries.iter().all(|entry| entry.receipt.is_none()));
        let first = receipt(1);
        runtime.index_remote_local_review(&first).unwrap();
        assert_eq!(runtime.store.revision(&stream(&runtime)).unwrap(), 18);
        runtime
            .store
            .append_with_outcome(
                &format!("result-local-review-{}", first.offer),
                0,
                "review",
                &first.encoded(),
            )
            .unwrap();
        assert_eq!(
            runtime.remote_local_reviews(0, Some(18)).unwrap().entries[0].receipt,
            Some(first.clone())
        );
        runtime.index_remote_local_review(&receipt(19)).unwrap();
        let last = runtime
            .remote_local_reviews(16, Some(page.snapshot))
            .unwrap();
        assert_eq!((last.entries.len(), last.next), (2, None));
        assert_eq!(
            runtime
                .remote_local_reviews(18, None)
                .unwrap()
                .entries
                .len(),
            1
        );
        assert!(runtime.remote_local_reviews(20, None).is_err());
        assert!(runtime.remote_local_reviews(0, Some(20)).is_err());
        let mut conflict = first.clone();
        conflict.mapping = RecordDigest::from_bytes([99; 32]);
        assert!(runtime.index_remote_local_review(&conflict).is_err());
        drop(runtime);
        let mut runtime = Runtime::open(
            crate::fleet::FleetStore::open(f.path.join("coordinator.sqlite")).unwrap(),
            "objective",
        )
        .unwrap();
        assert_eq!(
            runtime.remote_local_reviews(0, Some(18)).unwrap().entries[0].receipt,
            Some(first)
        );
        runtime
            .store
            .append_with_outcome(&stream(&runtime), 19, "malformed", "{}")
            .unwrap();
        assert!(runtime.remote_local_reviews(19, None).is_err());
        assert_eq!(
            runtime.store.revision(&stream(&runtime)).unwrap(),
            20,
            "read must not repair malformed history"
        );
    }
}
