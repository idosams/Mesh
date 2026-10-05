//! Ordered publication claims, not verified human authority or a workspace admission capability.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Publication {
    pub(super) request: RecordDigest,
    pub(super) revision: u64,
    pub(super) previous: RecordDigest,
    pub(super) review: RecordDigest,
    pub(super) receipt: RecordDigest,
    pub(super) result: RecordDigest,
    pub(super) credential: RecordDigest,
    pub(super) challenge: RecordDigest,
}
impl Publication {
    pub(super) fn decode(body: &Json) -> Result<Self> {
        fields(
            body,
            &[
                "request",
                "revision",
                "previous",
                "review",
                "receipt",
                "result",
                "credential",
                "challenge",
            ],
        )?;
        let revision = number(value(body, "revision")?)?;
        if revision == 0 {
            return Err(InvalidDependencyHistory);
        }
        Ok(Self {
            request: digest(value(body, "request")?, false)?,
            revision,
            previous: digest(value(body, "previous")?, true)?,
            review: digest(value(body, "review")?, false)?,
            receipt: digest(value(body, "receipt")?, false)?,
            result: digest(value(body, "result")?, false)?,
            credential: digest(value(body, "credential")?, false)?,
            challenge: digest(value(body, "challenge")?, false)?,
        })
    }
}
