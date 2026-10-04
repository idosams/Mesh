//! Required destination provenance. Decoding establishes no access or cross-store authority.
use super::*;

/// Exact staged starting intent. Configuration digests bind canonical bytes retained by `staged`.
/// The destination work identity is distinct from this installation's enrollment project identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConsumedStart {
    pub(super) request: RecordDigest,
    pub(super) owner: NativeDependencyBinding,
    pub(super) destination: Work,
    pub(super) source: Input,
    pub(super) grant: RecordDigest,
    pub(super) bindings: (RecordDigest, RecordDigest),
    pub(super) configuration: RecordDigest,
    pub(super) prospective: RecordDigest,
    pub(super) closure: RecordDigest,
    pub(super) operation: RecordDigest,
    pub(super) staged: RecordDigest,
}
impl ConsumedStart {
    pub(super) fn decode(body: &Json, local: NativeDependencyBinding) -> Result<Self> {
        fields(
            body,
            &[
                "request",
                "owner",
                "destination",
                "source",
                "grant",
                "bindings",
                "configuration",
                "prospective",
                "closure",
                "operation",
                "staged",
            ],
        )?;
        let Json::Array(owner) = value(body, "owner")? else {
            return Err(InvalidDependencyHistory);
        };
        if owner.len() != 3 {
            return Err(InvalidDependencyHistory);
        }
        let owner = NativeDependencyBinding {
            authority: digest(&owner[0], false)?,
            project: digest(&owner[1], false)?,
            installation: digest(&owner[2], false)?,
        };
        let destination = work(value(body, "destination")?)?;
        let source = input(value(body, "source")?)?;
        let Json::Array(bindings) = value(body, "bindings")? else {
            return Err(InvalidDependencyHistory);
        };
        if bindings.len() != 2
            || destination.1 != local.installation
            || destination.0 == source.0 .0
            || destination.1 == source.0 .1
            || owner.authority == local.authority
            || owner.project == local.project
            || owner.installation == local.installation
        {
            return Err(InvalidDependencyHistory);
        }
        Ok(Self {
            request: digest(value(body, "request")?, false)?,
            owner,
            destination,
            source,
            grant: digest(value(body, "grant")?, false)?,
            bindings: (digest(&bindings[0], false)?, digest(&bindings[1], false)?),
            configuration: digest(value(body, "configuration")?, false)?,
            prospective: digest(value(body, "prospective")?, false)?,
            closure: digest(value(body, "closure")?, false)?,
            operation: digest(value(body, "operation")?, false)?,
            staged: digest(value(body, "staged")?, false)?,
        })
    }

    /// Direct immutable references, not a complete physical CAS collection oracle.
    pub(super) fn references(&self) -> [RecordDigest; 7] {
        [
            self.source.1,
            self.grant,
            self.configuration,
            self.prospective,
            self.closure,
            self.operation,
            self.staged,
        ]
    }
}

/// Destination acknowledgement links the exact start intent and immutable owner receipt.
/// The owner's full envelope and payload must still be independently replayed at native admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConsumedComplete {
    pub(super) start: RecordDigest,
    pub(super) owner_receipt: RecordDigest,
}
impl ConsumedComplete {
    pub(super) fn decode(body: &Json) -> Result<Self> {
        fields(body, &["start", "owner_receipt"])?;
        Ok(Self {
            start: digest(value(body, "start")?, false)?,
            owner_receipt: digest(value(body, "owner_receipt")?, false)?,
        })
    }
}
