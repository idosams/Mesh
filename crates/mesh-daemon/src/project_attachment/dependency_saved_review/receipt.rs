//! Exact historical receipt verification. This does not commit or authorize publication.
use super::*;
use crate::{
    dependency_policy::NativeReviewBinding, workspace::NativePrivateReviewHistory, TrustedReviewers,
};
use mesh_approval::{ExpectedHumanApproval, HumanApprovalReceipt};

pub(super) const MAX_RECEIPT_BYTES: usize = 65_536;

pub(super) fn check_receipt(
    history: &NativePrivateReviewHistory<'_>,
    binding: &NativeReviewBinding,
    bytes: &[u8],
    trusted: &TrustedReviewers,
) -> io::Result<ExpectedHumanApproval> {
    if bytes.is_empty() || bytes.len() > MAX_RECEIPT_BYTES {
        return Err(invalid(
            "native review receipt exceeds its bound or is empty",
        ));
    }
    let receipt = HumanApprovalReceipt::from_canonical_bytes(bytes).map_err(error)?;
    let carried = receipt.draft().expected();
    let credential = trusted
        .human_credential(carried.credential().id())
        .ok_or_else(|| invalid("native review receipt credential is not trusted"))?;
    if carried.challenge() == &[0; 32] {
        return Err(invalid("native review receipt challenge is empty"));
    }
    // Reconstruct from the owner-held binding and exact saved output. Carried context never
    // selects the work, bundle, canonical base, validation evidence or resulting head.
    let native = history.approval_context(binding).map_err(error)?;
    let expected = ExpectedHumanApproval::new(native, credential, *carried.challenge());
    mesh_approval::verify_human_approval_receipt(bytes, &expected).map_err(error)?;
    Ok(expected)
}
