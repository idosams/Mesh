//! Exact signed import intent and recovery. The journal decides completion; receipts never approve.
use super::{invalid, read_receipt, ProvisionedAttachment};
use crate::authenticated_changeset::CHANGESET_SIGNATURE_DOMAIN;
use crate::checkpoint_storage::{
    authenticated_checkpoint_identity, operation_checkpoint_signing_body,
    save_authenticated_checkpoint, AuthenticatedOperationCheckpointRequest,
};
use crate::fleet::{CandidateImportSigner, PreparedProjectCandidateImport};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::workspace::{HistoricalWorkspacePreview, OpenWorkspace};
use crate::TrustedReviewers;
use mesh_crypto::{DomainSeparator, Ed25519, SignatureScheme as _, SigningPayload};
use mesh_operations::{HeadDerivation, HeadId, TransitionCommitment};
use mesh_store::{RecordDigest, TailResidue};
use mesh_types::{Blake3, ContentDigest as _, PublicKey, Signature};
use std::fs;
use std::io::{self, Read as _};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

const RECORD: &str = "import.json";
const LIMIT: u64 = 131_072;
const DOMAIN: DomainSeparator = DomainSeparator::new("mesh.v0.fleet-project-import");
struct DerivedHead;
impl HeadDerivation for DerivedHead {
    fn resulting_head(&self, value: &TransitionCommitment) -> HeadId {
        HeadId::from_bytes(*Blake3::digest_bytes(&value.canonical_bytes()).as_bytes())
    }
}
fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn bytes<const N: usize>(value: &str) -> io::Result<[u8; N]> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("noncanonical import proof"));
    }
    let mut result = [0; N];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(error)?;
    }
    Ok(result)
}
fn text<'a>(value: &'a Json, field: &str) -> io::Result<&'a str> {
    value
        .get(field)
        .and_then(Json::as_text)
        .ok_or_else(|| invalid("missing import identity"))
}
fn hash(value: &Json) -> RecordDigest {
    RecordDigest::from_bytes(*Blake3::digest_bytes(value.encode().as_bytes()).as_bytes())
}
fn predecessor(candidate: &Json) -> io::Result<RecordDigest> {
    let provenance = candidate
        .get("provenance")
        .ok_or_else(|| invalid("missing candidate provenance"))?;
    Ok(RecordDigest::from_bytes(bytes(text(
        provenance,
        "source_version",
    )?)?))
}
fn expected_main(candidate: &Json) -> io::Result<Option<String>> {
    match candidate
        .get("provenance")
        .and_then(|p| p.get("expected_main"))
    {
        Some(Json::Null) => Ok(None),
        Some(Json::Text(value)) => {
            let _ = bytes::<32>(value)?;
            Ok(Some(value.clone()))
        }
        _ => Err(invalid("missing candidate base")),
    }
}
struct Receipt {
    statement: Json,
    signature: Signature,
    operation_signature: Signature,
    operation: RecordDigest,
}
impl Receipt {
    fn encoded(&self) -> String {
        Json::object([
            ("schema", Json::text("mesh.fleet-project-import-receipt/v1")),
            ("statement", self.statement.clone()),
            ("signature", Json::text(hex(self.signature.as_bytes()))),
        ])
        .encode()
    }
    fn read(
        root: &PinnedWorkspaceRoot,
        candidate: &Json,
        actor: PublicKey,
    ) -> io::Result<Option<Self>> {
        let file = match root.filesystem().inspect_entry(Path::new(RECORD)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > LIMIT
        {
            return Err(invalid("import receipt is not a bounded private record"));
        }
        let mut raw = String::new();
        file.take(LIMIT + 1).read_to_string(&mut raw)?;
        if raw.len() as u64 > LIMIT {
            return Err(invalid("import receipt exceeds limit"));
        }
        let value = Json::parse(&raw).map_err(error)?;
        let statement = value
            .get("statement")
            .ok_or_else(|| invalid("missing import statement"))?
            .clone();
        let receipt = Self {
            signature: Signature::from_bytes(bytes(text(&value, "signature")?)?),
            operation_signature: Signature::from_bytes(bytes(text(
                &statement,
                "operation_signature",
            )?)?),
            operation: RecordDigest::from_bytes(bytes(text(&statement, "operation")?)?),
            statement,
        };
        if receipt.encoded() != raw
            || receipt.statement.get("schema")
                != Some(&Json::text("mesh.fleet-project-import-statement/v1"))
            || receipt.statement.get("candidate") != Some(candidate)
            || text(&receipt.statement, "actor")? != hex(actor.as_bytes())
            || receipt.statement.get("source_version")
                != Some(&Json::text(predecessor(candidate)?.to_string()))
        {
            return Err(invalid("import receipt binding changed"));
        }
        Ed25519::verify(
            &actor,
            SigningPayload::new(DOMAIN, receipt.statement.encode().as_bytes()).as_bytes(),
            &receipt.signature,
        )
        .map_err(error)?;
        root.ensure_namespace_identity()?;
        Ok(Some(receipt))
    }
    fn outcome(
        &self,
        open: &OpenWorkspace,
        candidate: &Json,
        actor: PublicKey,
        snapshot: &HistoricalWorkspacePreview,
    ) -> io::Result<Json> {
        if open.tail() != TailResidue::Whole {
            return Err(invalid("import journal needs reconciliation"));
        }
        let committed = open.has_operation(&self.operation);
        if committed {
            open.verify_authenticated_operation(self.operation, actor, self.operation_signature)
                .map_err(error)?;
            let chain = open.linear_history(Some(self.operation)).map_err(error)?;
            if chain.iter().rev().nth(1).copied() != Some(predecessor(candidate)?) {
                return Err(invalid("import predecessor changed"));
            }
            let actual = open
                .historical_workspace_preview(self.operation)
                .map_err(error)?;
            let mut expected_dirs = snapshot
                .directories
                .iter()
                .map(|d| &d.path)
                .collect::<Vec<_>>();
            expected_dirs.sort();
            let mut actual_dirs = actual
                .directories
                .iter()
                .map(|d| &d.path)
                .collect::<Vec<_>>();
            actual_dirs.sort();
            let content = |view: &HistoricalWorkspacePreview| {
                view.files
                    .iter()
                    .map(|f| {
                        (
                            f.path.clone(),
                            (f.content_digest, f.byte_length, f.executable),
                        )
                    })
                    .collect::<std::collections::BTreeMap<_, _>>()
            };
            if actual_dirs != expected_dirs || content(&actual) != content(snapshot) {
                return Err(invalid("imported content differs from candidate"));
            }
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-project-import/v1")),
            ("candidate", candidate.clone()),
            (
                "receipt_digest",
                Json::text(hash(&Json::parse(&self.encoded()).map_err(error)?).to_string()),
            ),
            ("target", Json::text(self.operation.to_string())),
            (
                "state",
                Json::text(if committed { "imported" } else { "pending" }),
            ),
            ("approval_authority", Json::Bool(false)),
        ]))
    }
}
impl ProvisionedAttachment {
    pub(crate) fn inspect_fleet_import(
        &self,
        request: &str,
        candidate: &Json,
        snapshot: &HistoricalWorkspacePreview,
        actor: PublicKey,
        trusted: &TrustedReviewers,
    ) -> io::Result<Option<Json>> {
        self.attachment.with_review_history(
            self.metadata_path(),
            self.store.clone(),
            trusted,
            |open, _| {
                let allocation = self.retain_import_candidate(request, candidate, snapshot)?;
                Receipt::read(&allocation, candidate, actor)?
                    .map(|receipt| receipt.outcome(open, candidate, actor, snapshot))
                    .transpose()
            },
        )
    }
    pub(crate) fn commit_fleet_import(
        &self,
        request: &str,
        candidate: &Json,
        plan: PreparedProjectCandidateImport,
        signer: &dyn CandidateImportSigner,
        trusted: &TrustedReviewers,
    ) -> io::Result<Json> {
        self.commit_fleet_import_with(request, candidate, plan, signer, trusted, || Ok(()))
    }

    fn commit_fleet_import_with(
        &self,
        request: &str,
        candidate: &Json,
        plan: PreparedProjectCandidateImport,
        signer: &dyn CandidateImportSigner,
        trusted: &TrustedReviewers,
        before_append: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<Json> {
        self.attachment.with_review_history(
            self.metadata_path(),
            self.store.clone(),
            trusted,
            |open, store| {
                let actor = signer.public_key();
                let allocation =
                    self.retain_import_candidate(request, candidate, &plan.snapshot)?;
                let retained = Receipt::read(&allocation, candidate, actor)?;
                if let Some(receipt) = &retained {
                    let outcome = receipt.outcome(open, candidate, actor, &plan.snapshot)?;
                    if outcome.get("state") == Some(&Json::text("imported")) {
                        return Ok(outcome);
                    }
                }
                super::detachment::ensure_attached(store)?;
                let (configuration, _) = self.attachment.history_configuration(store, None)?;
                let line = super::capture_line::CaptureLine::load(store, open, &configuration)?;
                line.persist(store, &configuration)?;
                self.attachment
                    .enable_capture_line(store, &configuration, open, line.head)?;
                let context = plan.context();
                let verify_plan = |current: &OpenWorkspace| -> io::Result<()> {
                    if current.tail() != TailResidue::Whole
                        || plan.candidate != hash(candidate)
                        || plan.historical.target() != predecessor(candidate)?
                        || super::approval::main_head(current)?.map(|id| id.to_string())
                            != expected_main(candidate)?
                    {
                        return Err(invalid("candidate import base changed"));
                    }
                    let fresh = current
                        .prepare_historical_operations(
                            plan.historical.target(),
                            actor,
                            plan.historical.operations(),
                        )
                        .map_err(error)?;
                    if fresh.context() != plan.historical.context() {
                        return Err(invalid("candidate import context changed"));
                    }
                    Ok(())
                };
                verify_plan(open)?;
                let basis = open
                    .historical_authoring_basis(plan.historical.target(), actor)
                    .map_err(error)?;
                let make_request = |signature| {
                    AuthenticatedOperationCheckpointRequest::new(
                        basis.workspace_id,
                        basis.actor_id,
                        basis.session_id,
                        basis.actor_sequence,
                        basis.causal_parents.clone(),
                        basis.base_head,
                        basis.policy_epoch,
                        basis.hybrid_logical_time,
                        plan.historical.operations().to_vec(),
                        actor,
                        signature,
                    )
                };
                let receipt = match retained {
                    Some(receipt) => {
                        if receipt.statement.get("plan") != Some(&context) {
                            return Err(invalid("pending import plan changed"));
                        }
                        receipt
                    }
                    None => {
                        let unsigned = make_request(Signature::from_bytes([0; 64]));
                        let signature = signer
                            .sign(&SigningPayload::new(
                                CHANGESET_SIGNATURE_DOMAIN,
                                &operation_checkpoint_signing_body(&unsigned, &DerivedHead),
                            ))
                            .map_err(error)?;
                        let operation = authenticated_checkpoint_identity(
                            &make_request(signature),
                            &DerivedHead,
                        )
                        .map_err(error)?;
                        let statement = Json::object([
                            (
                                "schema",
                                Json::text("mesh.fleet-project-import-statement/v1"),
                            ),
                            ("candidate", candidate.clone()),
                            ("plan", context.clone()),
                            (
                                "source_version",
                                Json::text(plan.historical.target().to_string()),
                            ),
                            ("actor", Json::text(hex(actor.as_bytes()))),
                            ("operation", Json::text(operation.to_string())),
                            ("operation_signature", Json::text(hex(signature.as_bytes()))),
                        ]);
                        let proof = signer
                            .sign_import_provenance(&SigningPayload::new(
                                DOMAIN,
                                statement.encode().as_bytes(),
                            ))
                            .map_err(error)?;
                        Ed25519::verify(
                            &actor,
                            SigningPayload::new(DOMAIN, statement.encode().as_bytes()).as_bytes(),
                            &proof,
                        )
                        .map_err(error)?;
                        Receipt {
                            statement,
                            signature: proof,
                            operation_signature: signature,
                            operation,
                        }
                    }
                };
                let signed = make_request(receipt.operation_signature);
                if authenticated_checkpoint_identity(&signed, &DerivedHead).map_err(error)?
                    != receipt.operation
                {
                    return Err(invalid("pending import operation changed"));
                }
                // Signing may yield to native key custody. Reopen and revalidate before recording intent.
                self.project().ensure_current()?;
                store.ensure_namespace_identity()?;
                if read_receipt(store)? != self.project().receipt()?.encode()
                    || self.attachment.history_configuration(store, None)?.0 != configuration
                {
                    return Err(invalid("import attachment changed while signing"));
                }
                super::detachment::ensure_attached(store)?;
                let mut reopened = OpenWorkspace::open_attachment_store_with_trusted_reviewers(
                    self.metadata_path(),
                    store.clone(),
                    false,
                    trusted,
                )
                .map_err(error)?;
                verify_plan(&reopened)?;
                if super::capture_line::CaptureLine::load(store, &reopened, &configuration)? != line
                {
                    return Err(invalid("capture position changed while signing import"));
                }
                self.retain_import_candidate(request, candidate, &plan.snapshot)?;
                let encoded = receipt.encoded();
                if encoded.len() as u64 > LIMIT {
                    return Err(invalid("import receipt exceeds limit"));
                }
                match Receipt::read(&allocation, candidate, actor)? {
                    Some(existing) if existing.encoded() == encoded => {}
                    Some(_) => return Err(invalid("import receipt changed before append")),
                    None => {
                        allocation.filesystem().write_new_file(
                            Path::new(RECORD),
                            encoded.as_bytes(),
                            fs::Permissions::from_mode(0o600),
                        )?;
                        allocation.sync()?;
                    }
                }
                allocation.ensure_namespace_identity()?;
                // Fault boundary after durable intent, before any imported operation is appended.
                before_append()?;
                let cas = mesh_cas::Cas::with_filesystem(
                    self.metadata_path().to_owned(),
                    store.filesystem(),
                )
                .map_err(error)?;
                save_authenticated_checkpoint(
                    &mut reopened,
                    &cas,
                    signed,
                    plan.files,
                    &DerivedHead,
                )
                .map_err(error)?;
                let saved = OpenWorkspace::open_attachment_store_with_trusted_reviewers(
                    self.metadata_path(),
                    store.clone(),
                    false,
                    trusted,
                )
                .map_err(error)?;
                self.retain_import_candidate(request, candidate, &plan.snapshot)?;
                let verified = Receipt::read(&allocation, candidate, actor)?
                    .ok_or_else(|| invalid("import receipt missing after append"))?;
                if verified.encoded() != encoded {
                    return Err(invalid("import receipt changed after append"));
                }
                verified.outcome(&saved, candidate, actor, &plan.snapshot)
            },
        )
    }
}

#[cfg(test)]
#[path = "candidate_import_tests.rs"]
mod tests;
