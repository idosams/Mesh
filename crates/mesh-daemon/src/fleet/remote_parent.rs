//! Stable local-parent review selection at the first authenticated remote launch claim.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ParentReview {
    Missing,
    Ambiguous,
    Selected {
        checkpoint: String,
        bundle: RecordDigest,
    },
}
impl State {
    pub(super) fn select_remote_parent(&self, lane: &str) -> Result<Option<ParentReview>, Error> {
        let lane = self.lanes.get(lane).ok_or(Error::Refused("lane-missing"))?;
        let Some(parent) = lane.parent.as_deref() else {
            return Ok(None);
        };
        let mut matches = self.checkpoints.iter().filter_map(|(id, cp)| {
            (cp.lane == parent
                && cp
                    .result
                    .as_ref()
                    .is_some_and(|r| r.complete && r.version == lane.base))
            .then_some(cp.review)
            .flatten()
            .map(|bundle| (id, bundle))
        });
        Ok(Some(match (matches.next(), matches.next()) {
            (None, _) => ParentReview::Missing,
            (Some(_), Some(_)) => ParentReview::Ambiguous,
            (Some((id, bundle)), None) => ParentReview::Selected {
                checkpoint: id.clone(),
                bundle,
            },
        }))
    }

    #[cfg(any(test, target_os = "macos"))]
    pub(super) fn remote_parent_review(&self, lane: &str) -> Result<(&str, RecordDigest), Error> {
        match self.remote_parent_reviews.get(lane) {
            Some(ParentReview::Selected { checkpoint, bundle }) => Ok((checkpoint, *bundle)),
            Some(ParentReview::Ambiguous) => {
                Err(Error::Refused("fleet-remote-parent-review-ambiguous"))
            }
            Some(ParentReview::Missing) => {
                Err(Error::Refused("fleet-remote-parent-review-missing"))
            }
            None => Err(Error::Refused("fleet-remote-parent-admission-missing")),
        }
    }
}
