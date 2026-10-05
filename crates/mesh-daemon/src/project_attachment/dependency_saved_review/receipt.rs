//! Exact historical receipt verification. This does not commit or authorize publication.
use super::*;
use crate::{
    dependency_policy::NativeReviewBinding, workspace::NativePrivateReviewHistory, TrustedReviewers,
};
use mesh_approval::ExpectedHumanApproval;

pub(super) const MAX_RECEIPT_BYTES: usize = 65_536;

pub(super) fn check_receipt(
    history: &NativePrivateReviewHistory<'_>,
    binding: &NativeReviewBinding,
    bytes: &[u8],
    trusted: &TrustedReviewers,
) -> io::Result<ExpectedHumanApproval> {
    history
        .check_receipt(binding, bytes, trusted)
        .map_err(error)
}
