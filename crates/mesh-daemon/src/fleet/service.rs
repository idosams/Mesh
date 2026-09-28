//! Native fleet host and scoped agent calls. Agent input never selects filesystem destinations.
//!
//! The native host creates objectives, dispatches runs and issues credentials. This agent surface
//! inspects its bound workspace, observes/delegates children, captures private work and submits
//! immutable reviews. No agent call can approve or publish a version.
//! Credentials are ephemeral, redacted from Debug, and never written to the event ledger. A
//! restart requires native reauthorization; disconnect never implies a worker has stopped.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use super::workspace::{LaneWorkspace, VersionInput};
use super::{Command, Lane, RunState, Runtime};
use crate::ipc::{Json, Unavailable, WorkspaceSummary};
use crate::{CheckpointRuntimeParameters, ProtectedWorkspaceRoot, TrustedReviewers};
use mesh_store::RecordDigest;
use mesh_types::{Blake3, ContentDigest};

pub use crate::CheckpointSigner;

/// Private allocation policy implemented by the native host, never supplied over agent IPC.
pub trait LaneAllocator: Send + Sync {
    /// Create a folder for a service-generated lane identity and verified immutable source.
    fn allocate(&self, lane: &str, input: &VersionInput) -> Result<LaneWorkspace, Unavailable>;
    /// Native-only attached source allocation. Custom allocators must opt in explicitly.
    fn allocate_attached(
        &self,
        _lane: &str,
        _source: &crate::project_attachment::ProvisionedAttachment,
        _version: RecordDigest,
    ) -> Result<LaneWorkspace, Unavailable> {
        Err(refusal("fleet-attachment-allocation-unsupported"))
    }
}

/// Descriptor-pinned native allocation root.
pub struct NativeLaneAllocator {
    path: std::path::PathBuf,
    root: crate::root_authority::PinnedWorkspaceRoot,
    reviewers: TrustedReviewers,
    checkpoint: CheckpointRuntimeParameters,
    protected: Vec<ProtectedWorkspaceRoot>,
}
impl NativeLaneAllocator {
    /// Admit a native-owned existing private directory. Its lifetime pins the directory object.
    pub fn open(
        root: &Path,
        reviewers: TrustedReviewers,
        checkpoint: CheckpointRuntimeParameters,
        protected: Vec<ProtectedWorkspaceRoot>,
    ) -> Result<Self, Unavailable> {
        use std::os::unix::fs::PermissionsExt as _;
        let path = root.to_path_buf();
        let root = crate::root_authority::PinnedWorkspaceRoot::open(path.clone())
            .map_err(|_| refusal("fleet-root-unavailable"))?;
        let directory = root
            .try_clone_directory()
            .map_err(|_| refusal("fleet-root-unavailable"))?;
        if directory
            .metadata()
            .map_err(|_| refusal("fleet-root-unavailable"))?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(refusal("fleet-root-not-private"));
        }
        Ok(Self {
            path,
            root,
            reviewers,
            checkpoint,
            protected,
        })
    }
}
impl LaneAllocator for NativeLaneAllocator {
    fn allocate_attached(
        &self,
        lane: &str,
        source: &crate::project_attachment::ProvisionedAttachment,
        version: RecordDigest,
    ) -> Result<LaneWorkspace, Unavailable> {
        if !lane.starts_with("lane-")
            || lane.len() != 69
            || !lane[5..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(refusal("fleet-allocation-identity"));
        }
        source
            .validate_lane_version(&version.to_string())
            .map_err(|_| refusal("fleet-source-version-unavailable"))?;
        let original = source
            .protected_source()
            .map_err(|_| refusal("fleet-source-unavailable"))?;
        if self
            .root
            .is_within(original)
            .map_err(|_| refusal("fleet-root-unavailable"))?
        {
            return Err(refusal("fleet-allocation-inside-source"));
        }
        for protected in &self.protected {
            if self
                .root
                .is_within(*protected)
                .map_err(|_| refusal("fleet-root-unavailable"))?
            {
                return Err(refusal("fleet-allocation-inside-protected"));
            }
        }
        let child = self
            .root
            .create_child_directory(std::ffi::OsStr::new(lane))
            .map_err(|_| refusal("fleet-allocation-needs-recovery"))?;
        let mut protected = self.protected.clone();
        protected.push(original);
        LaneWorkspace::from_attachment(
            source,
            version,
            &child,
            &self.path.join(lane),
            self.reviewers.clone(),
            self.checkpoint,
            &protected,
        )
    }

    fn allocate(&self, lane: &str, input: &VersionInput) -> Result<LaneWorkspace, Unavailable> {
        // Names are generated by the service, but keep the native seam closed independently.
        if !lane.starts_with("lane-")
            || lane.len() != 69
            || !lane[5..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(refusal("fleet-allocation-identity"));
        }
        let child = self
            .root
            .create_child_directory(std::ffi::OsStr::new(lane))
            .map_err(|_| refusal("fleet-allocation-needs-recovery"))?;
        let (device, inode) = child
            .identity()
            .map_err(|_| refusal("fleet-allocation-unavailable"))?;
        let identity =
            ProtectedWorkspaceRoot::from_directory_token(&format!("{device:016x}:{inode:016x}"))
                .map_err(|_| refusal("fleet-allocation-unavailable"))?;
        // Every native writer pins and checks this exact parent before creating any entry.
        let result = LaneWorkspace::fork_inner(
            input,
            &self.path.join(lane).join("workspace.mesh"),
            &self.protected,
            self.reviewers.clone(),
            self.checkpoint,
            Some(identity),
        )?;
        child
            .ensure_identity(device, inode)
            .map_err(|_| refusal("fleet-allocation-unavailable"))?;
        Ok(result)
    }
}

/// A bearer credential for one native-issued actor/run session. Never include it in prompts/logs.
pub struct AgentCredential(String);
impl AgentCredential {
    /// Pass only to the trusted local MCP bridge through the provider's private configuration.
    pub fn transport_value(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for AgentCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentCredential([redacted])")
    }
}

#[derive(Clone)]
struct Grant {
    lane: String,
    run: String,
    actor: String,
    session: String,
    generation: String,
    signer: Option<Arc<dyn CheckpointSigner>>,
}
struct Inner {
    runtime: Runtime,
    workspaces: BTreeMap<String, Arc<LaneWorkspace>>,
    grants: BTreeMap<String, Grant>,
}
/// A pinned saved result. Native readers verify every member against durable checkpoint history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedReviewSelection {
    lane: String,
    checkpoint: String,
    version: RecordDigest,
    bundle: RecordDigest,
}
impl SavedReviewSelection {
    /// Parse bounded identities. Parsing alone grants no workspace or review authority.
    pub fn new(
        lane: &str,
        checkpoint: &str,
        version: &str,
        bundle: &str,
    ) -> Result<Self, Unavailable> {
        super::id_valid(lane).map_err(runtime_error)?;
        super::id_valid(checkpoint).map_err(runtime_error)?;
        let digest = |value: &str| {
            let parsed = RecordDigest::parse_hex(value)
                .map_err(|_| refusal("fleet-review-identity-invalid"))?;
            if parsed.to_string() != value {
                return Err(refusal("fleet-review-identity-invalid"));
            }
            Ok(parsed)
        };
        Ok(Self {
            lane: lane.into(),
            checkpoint: checkpoint.into(),
            version: digest(version)?,
            bundle: digest(bundle)?,
        })
    }
    /// Exact immutable selection for native-to-renderer correlation. It conveys no approval power.
    pub fn to_json(&self) -> Json {
        Json::object([
            ("lane", Json::text(&self.lane)),
            ("checkpoint", Json::text(&self.checkpoint)),
            ("version", Json::text(self.version.to_string())),
            ("bundle", Json::text(self.bundle.to_string())),
        ])
    }
}

/// One objective's native host. All mutation methods other than `agent_call` are native-only APIs.
pub struct FleetService {
    inner: Mutex<Inner>,
    allocator: Arc<dyn LaneAllocator>,
    providers: BTreeSet<String>,
    host: String,
}
impl std::fmt::Debug for FleetService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FleetService").finish_non_exhaustive()
    }
}
impl FleetService {
    /// Compose a previously initialized durable objective with native allocation/provider policy.
    pub fn new(
        mut runtime: Runtime,
        allocator: Arc<dyn LaneAllocator>,
        providers: BTreeSet<String>,
    ) -> Result<Self, Unavailable> {
        runtime.refresh().map_err(runtime_error)?;
        if runtime.state().goal.is_none()
            || providers.is_empty()
            || providers.len() > 16
            || providers.iter().any(|p| super::id_valid(p).is_err())
        {
            return Err(refusal("fleet-host-configuration"));
        }
        let mut host = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut host))
            .map_err(|_| refusal("fleet-host-identity-unavailable"))?;
        Ok(Self {
            inner: Mutex::new(Inner {
                runtime,
                workspaces: BTreeMap::new(),
                grants: BTreeMap::new(),
            }),
            allocator,
            providers,
            host: RecordDigest::from_bytes(host).to_string(),
        })
    }
    fn lock(&self) -> Result<MutexGuard<'_, Inner>, Unavailable> {
        self.inner
            .lock()
            .map_err(|_| refusal("fleet-host-needs-recovery"))
    }

    /// Native routing identity; credentials remain scoped to this objective.
    pub fn objective(&self) -> Result<String, Unavailable> {
        Ok(self.lock()?.runtime.objective().to_owned())
    }

    /// Create and allocate a root lane from a native-authorized version. Not exposed to agents.
    pub fn create_root(
        &self,
        request: &str,
        goal: &str,
        provider: &str,
        input: &VersionInput,
    ) -> Result<String, Unavailable> {
        let mut inner = self.lock()?;
        let lane = lane_identity(inner.runtime.objective(), "root", request)?;
        self.create(&mut inner, &lane, None, goal, provider, input)?;
        Ok(lane)
    }

    /// Allocate an exact saved attachment into an additional managed lane. Native callers retain
    /// the registration authority; no agent request can select an arbitrary source or destination.
    pub fn create_root_from_attachment(
        &self,
        request: &str,
        goal: &str,
        provider: &str,
        source: &crate::project_attachment::ProvisionedAttachment,
        version: RecordDigest,
    ) -> Result<String, Unavailable> {
        if !self.providers.contains(provider) {
            return Err(refusal("fleet-provider-not-authorized"));
        }
        source
            .validate_lane_version(&version.to_string())
            .map_err(|_| refusal("fleet-source-version-unavailable"))?;
        let mut inner = self.lock()?;
        let lane = lane_identity(inner.runtime.objective(), "root", request)?;
        inner
            .runtime
            .record(
                &format!("create-{lane}"),
                Command::CreateAttachedLane {
                    id: lane.clone(),
                    project: source.id().into(),
                    goal: goal.into(),
                    provider: provider.into(),
                    base: version,
                },
            )
            .map_err(runtime_error)?;
        if inner.runtime.state().lanes[&lane].workspace.is_some() {
            if !inner.workspaces.contains_key(&lane) {
                return Err(refusal("fleet-lane-needs-reattachment"));
            }
            return Ok(lane);
        }
        let allocated = Arc::new(self.allocator.allocate_attached(&lane, source, version)?);
        inner
            .runtime
            .record(
                &format!("allocate-{lane}"),
                Command::BindWorkspace {
                    lane: lane.clone(),
                    binding: allocated.binding().clone(),
                },
            )
            .map_err(runtime_error)?;
        inner.workspaces.insert(lane.clone(), allocated);
        Ok(lane)
    }

    fn create(
        &self,
        inner: &mut Inner,
        lane: &str,
        parent: Option<String>,
        goal: &str,
        provider: &str,
        input: &VersionInput,
    ) -> Result<(), Unavailable> {
        if !self.providers.contains(provider) {
            return Err(refusal("fleet-provider-not-authorized"));
        }
        inner
            .runtime
            .record(
                &format!("create-{lane}"),
                Command::CreateLane {
                    id: lane.into(),
                    parent,
                    goal: goal.into(),
                    provider: provider.into(),
                    base: input.version,
                },
            )
            .map_err(runtime_error)?;
        self.finish_allocation(inner, lane, input)
    }

    fn finish_allocation(
        &self,
        inner: &mut Inner,
        lane: &str,
        input: &VersionInput,
    ) -> Result<(), Unavailable> {
        if inner.runtime.state().lanes[lane].workspace.is_some() {
            // Do not recreate a lost or replaced context. Reattachment requires native recovery.
            if !inner.workspaces.contains_key(lane) {
                return Err(refusal("fleet-lane-needs-reattachment"));
            }
            return Ok(());
        }
        let allocated = Arc::new(self.allocator.allocate(lane, input)?);
        inner
            .runtime
            .record(
                &format!("allocate-{lane}"),
                Command::BindWorkspace {
                    lane: lane.into(),
                    binding: allocated.binding().clone(),
                },
            )
            .map_err(runtime_error)?;
        inner.workspaces.insert(lane.into(), allocated);
        Ok(())
    }

    /// Record a scheduler decision or adapter observation. Never route untrusted command enums here.
    pub fn native_command(&self, request: &str, command: Command) -> Result<(), Unavailable> {
        self.lock()?
            .runtime
            .record(request, command)
            .map_err(runtime_error)?;
        Ok(())
    }

    /// Issue/rotate a credential for the current run, acquiring its exact native custody generation.
    /// Native provider setup supplies actor/session identities; the worker cannot choose them.
    pub fn grant(
        &self,
        lane: &str,
        run: &str,
        actor: &str,
        session: &str,
    ) -> Result<AgentCredential, Unavailable> {
        self.grant_inner(lane, run, actor, session, None)
    }

    /// Issue a capture-enabled session whose actor identity is the native signer's actual key.
    pub fn grant_with_signer(
        &self,
        lane: &str,
        run: &str,
        session: &str,
        signer: Arc<dyn CheckpointSigner>,
    ) -> Result<AgentCredential, Unavailable> {
        let actor = RecordDigest::from_bytes(*signer.public_key().as_bytes()).to_string();
        self.grant_inner(lane, run, &actor, session, Some(signer))
    }

    fn grant_inner(
        &self,
        lane: &str,
        run: &str,
        actor: &str,
        session: &str,
        signer: Option<Arc<dyn CheckpointSigner>>,
    ) -> Result<AgentCredential, Unavailable> {
        super::id_valid(actor).map_err(runtime_error)?;
        super::id_valid(session).map_err(runtime_error)?;
        let mut random = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|_| refusal("fleet-credential-unavailable"))?;
        let token = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        ensure_run(&inner, lane, run)?;
        let workspace = inner
            .workspaces
            .get(lane)
            .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))?;
        let state = exact_state(workspace)?;
        let previous = inner
            .grants
            .values()
            .find(|g| g.lane == lane && g.run == run);
        let generation = if let Some(previous) = previous {
            verify_custody(workspace, &state, &previous.generation)?;
            previous.generation.clone()
        } else {
            workspace
                .daemon()
                .acquire_workspace_agent_custody(
                    &state.root,
                    &state.digest,
                    &state.installation,
                    false,
                    None,
                )
                .map_err(|_| refusal("fleet-lane-custody-unavailable"))?
        };
        inner.grants.retain(|_, grant| grant.lane != lane);
        inner.grants.insert(
            token_key(&token),
            Grant {
                lane: lane.into(),
                run: run.into(),
                actor: actor.into(),
                session: session.into(),
                generation,
                signer,
            },
        );
        Ok(AgentCredential(token))
    }

    /// Revoke the session without claiming its process stopped or releasing native custody.
    pub fn revoke(&self, credential: &AgentCredential) -> Result<(), Unavailable> {
        self.lock()?
            .grants
            .remove(&token_key(credential.transport_value()));
        Ok(())
    }

    /// Claim and launch the configured Codex adapter for one native-issued session.
    /// This native-only entry point cannot be reached through agent or renderer IPC.
    /// Any uncertain launch stays claimed and requires reconciliation before another attempt.
    pub fn start_codex(
        &self,
        credential: &AgentCredential,
        adapter: &super::provider::CodexAdapter,
        endpoint: &Path,
    ) -> Result<super::provider::CodexProcess, Unavailable> {
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        let grant = inner
            .grants
            .get(&token_key(credential.transport_value()))
            .cloned()
            .ok_or_else(|| refusal("fleet-session-refused"))?;
        ensure_run(&inner, &grant.lane, &grant.run)?;
        let lane = &inner.runtime.state().lanes[&grant.lane];
        if lane.provider != "codex" {
            return Err(refusal("fleet-provider-mismatch"));
        }
        let goal = lane.goal.clone();
        let workspace = inner
            .workspaces
            .get(&grant.lane)
            .cloned()
            .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))?;
        let state = exact_state(&workspace)?;
        let verified = workspace
            .daemon()
            .verified_managed_workspace_path(&state.root, &state.digest, &state.installation)
            .map_err(|_| refusal("fleet-lane-identity-changed"))?;
        let _authority = workspace
            .daemon()
            .lock_workspace_agent_setup(
                &state.root,
                &state.digest,
                &state.installation,
                &grant.generation,
            )
            .map_err(|_| refusal("fleet-session-custody-changed"))?;
        // Reject a replay before record()'s normal idempotency recovery can grant another spawn.
        let run = inner.runtime.state().lanes[&grant.lane]
            .runs
            .last()
            .ok_or_else(|| refusal("fleet-run-not-active"))?;
        if run.launch_owner.is_some() || run.state != RunState::Launching {
            return Err(refusal("launch-needs-reconciliation"));
        }
        inner
            .runtime
            .record(
                &format!("launch-{}", token_key(&grant.run)),
                super::Command::ClaimLaunch {
                    lane: grant.lane.clone(),
                    run: grant.run.clone(),
                    owner: self.host.clone(),
                },
            )
            .map_err(runtime_error)?;
        verified
            .ensure_current()
            .map_err(|_| refusal("fleet-lane-identity-changed"))?;
        let mut process = adapter
            .spawn(
                verified.path(),
                endpoint,
                inner.runtime.objective(),
                credential,
                &goal,
            )
            .map_err(|_| refusal("fleet-provider-launch-needs-reconciliation"))?;
        if verified.ensure_current().is_err() {
            process.abort_direct();
            return Err(refusal("fleet-lane-identity-changed"));
        }
        if inner
            .runtime
            .record(
                &format!("running-{}", token_key(&grant.run)),
                super::Command::Observe {
                    lane: grant.lane.clone(),
                    run: grant.run.clone(),
                    state: RunState::Running,
                },
            )
            .is_err()
        {
            process.abort_direct();
            return Err(refusal("fleet-provider-launch-needs-reconciliation"));
        }
        Ok(process)
    }

    /// Bounded agent entry point. A token binds the caller; there is no caller-supplied lane/path.
    pub fn agent_call(
        &self,
        credential: &str,
        action: &str,
        arguments: &Json,
    ) -> Result<Json, Unavailable> {
        if credential.len() != 64 || !credential.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(refusal("fleet-session-refused"));
        }
        let mut inner = self.lock()?;
        let grant = inner
            .grants
            .get(&token_key(credential))
            .cloned()
            .ok_or_else(|| refusal("fleet-session-refused"))?;
        inner.runtime.refresh().map_err(runtime_error)?;
        ensure_run(&inner, &grant.lane, &grant.run)?;
        let workspace = inner
            .workspaces
            .get(&grant.lane)
            .cloned()
            .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))?;
        let state = exact_state(&workspace)?;
        verify_custody(&workspace, &state, &grant.generation)?;
        match action {
            "submit_review" => {
                exact_fields(arguments, &["checkpoint"])?;
                let id = field(arguments, "checkpoint")?;
                let checkpoint = inner
                    .runtime
                    .state()
                    .checkpoints
                    .get(id)
                    .ok_or_else(|| refusal("fleet-checkpoint-unavailable"))?;
                if checkpoint.lane != grant.lane
                    || checkpoint.origin.actor != grant.actor
                    || checkpoint.origin.session != grant.session
                    || checkpoint.origin.run != grant.run
                    || checkpoint.origin.generation != grant.generation
                {
                    return Err(refusal("fleet-checkpoint-not-in-session"));
                }
                let result = checkpoint
                    .result
                    .as_ref()
                    .filter(|result| result.complete)
                    .ok_or_else(|| refusal("fleet-checkpoint-incomplete"))?;
                let version = result.version;
                if let Some(bundle) = checkpoint.review {
                    return Ok(review_summary(id, version, bundle));
                }
                let signer = grant
                    .signer
                    .as_ref()
                    .ok_or_else(|| refusal("fleet-checkpoint-signer-unavailable"))?;
                let public = signer.public_key();
                if RecordDigest::from_bytes(*public.as_bytes()).to_string() != grant.actor {
                    return Err(refusal("fleet-checkpoint-signer-changed"));
                }
                let bundle = workspace.daemon().submit_agent_saved_review(
                    crate::AgentWorkspaceCheckpointRequest {
                        root: &state.root,
                        digest: &state.digest,
                        installation: &state.installation,
                        generation: &grant.generation,
                    },
                    version,
                    public,
                )?;
                inner
                    .runtime
                    .record(
                        &format!("review-{id}"),
                        Command::SubmitReview {
                            checkpoint: id.into(),
                            bundle,
                        },
                    )
                    .map_err(runtime_error)?;
                Ok(review_summary(id, version, bundle))
            }
            "checkpoint" => {
                exact_fields(arguments, &["request"])?;
                let request = field(arguments, "request")?;
                let id = format!(
                    "checkpoint-{}",
                    &lane_identity(inner.runtime.objective(), &grant.lane, request)?[5..]
                );
                let origin = super::AgentOrigin {
                    actor: grant.actor.clone(),
                    session: grant.session.clone(),
                    run: grant.run.clone(),
                    generation: grant.generation.clone(),
                };
                if let Some(previous) = inner.runtime.state().checkpoints.get(&id) {
                    if previous.lane != grant.lane || previous.origin != origin {
                        return Err(refusal("fleet-checkpoint-request-conflict"));
                    }
                    return previous
                        .result
                        .as_ref()
                        .map(|result| checkpoint_summary(&id, result))
                        .ok_or_else(|| refusal("fleet-checkpoint-needs-recovery"));
                }
                let signer = grant
                    .signer
                    .as_ref()
                    .ok_or_else(|| refusal("fleet-checkpoint-signer-unavailable"))?;
                let public = signer.public_key();
                if RecordDigest::from_bytes(*public.as_bytes()).to_string() != grant.actor {
                    return Err(refusal("fleet-checkpoint-signer-changed"));
                }
                {
                    let _authority = workspace
                        .daemon()
                        .lock_workspace_agent_setup(
                            &state.root,
                            &state.digest,
                            &state.installation,
                            &grant.generation,
                        )
                        .map_err(|_| refusal("fleet-session-custody-changed"))?;
                    inner
                        .runtime
                        .record(
                            &format!("begin-{id}"),
                            Command::BeginCheckpoint {
                                id: id.clone(),
                                lane: grant.lane.clone(),
                                origin,
                                input_digest: state.digest.clone(),
                            },
                        )
                        .map_err(runtime_error)?;
                }
                let report = workspace
                    .daemon()
                    .checkpoint_agent_workspace(
                        crate::AgentWorkspaceCheckpointRequest {
                            root: &state.root,
                            digest: &state.digest,
                            installation: &state.installation,
                            generation: &grant.generation,
                        },
                        public,
                        |payload| signer.sign(payload),
                    )
                    .map_err(|_| refusal("fleet-checkpoint-needs-recovery"))?;
                let result = super::CheckpointResult {
                    complete: report.complete,
                    version: report
                        .workspace
                        .workspace_versions
                        .last()
                        .ok_or_else(|| refusal("fleet-checkpoint-version-unavailable"))?
                        .operation(),
                    workspace_digest: report.workspace.digest.clone(),
                    saved_changes: report.saved_changes.len() as u64,
                    issue: report.issue.map(str::to_owned),
                };
                inner
                    .runtime
                    .record(
                        &format!("finish-{id}"),
                        Command::FinishCheckpoint {
                            id: id.clone(),
                            result: result.clone(),
                        },
                    )
                    .map_err(runtime_error)?;
                Ok(checkpoint_summary(&id, &result))
            }
            "context" => {
                exact_fields(arguments, &[])?;
                Ok(Json::object([
                    ("objective", Json::text(inner.runtime.objective())),
                    ("lane", Json::text(&grant.lane)),
                    ("run", Json::text(&grant.run)),
                    ("actor", Json::text(&grant.actor)),
                    ("session", Json::text(&grant.session)),
                    ("generation", Json::text(&grant.generation)),
                    (
                        "goal",
                        Json::text(&inner.runtime.state().lanes[&grant.lane].goal),
                    ),
                    ("workspace", state.to_json()),
                ]))
            }
            "children" => {
                exact_fields(arguments, &[])?;
                let children = inner
                    .runtime
                    .state()
                    .lanes
                    .values()
                    .filter(|lane| lane.parent.as_deref() == Some(&grant.lane))
                    .map(lane_summary)
                    .collect();
                Ok(Json::object([
                    ("revision", Json::Number(inner.runtime.state().revision)),
                    ("lanes", Json::Array(children)),
                ]))
            }
            "delegate" => {
                exact_fields(arguments, &["request", "goal", "provider", "version"])?;
                let request = field(arguments, "request")?;
                let goal = field(arguments, "goal")?;
                let provider = field(arguments, "provider")?;
                let version = RecordDigest::parse_hex(field(arguments, "version")?)
                    .map_err(|_| refusal("fleet-version-invalid"))?;
                if !state
                    .workspace_versions
                    .iter()
                    .any(|saved| saved.operation() == version)
                {
                    return Err(refusal("fleet-version-not-in-lane"));
                }
                let child = lane_identity(inner.runtime.objective(), &grant.lane, request)?;
                let source = VersionInput {
                    root: state.root,
                    digest: state.digest,
                    installation: state.installation,
                    version,
                };
                if !self.providers.contains(provider) {
                    return Err(refusal("fleet-provider-not-authorized"));
                }
                {
                    let _authority = workspace
                        .daemon()
                        .lock_workspace_agent_setup(
                            &source.root,
                            &source.digest,
                            &source.installation,
                            &grant.generation,
                        )
                        .map_err(|_| refusal("fleet-session-custody-changed"))?;
                    inner
                        .runtime
                        .record(
                            &format!("create-{child}"),
                            Command::Delegate {
                                id: child.clone(),
                                parent: grant.lane.clone(),
                                goal: goal.into(),
                                provider: provider.into(),
                                base: version,
                                origin: super::AgentOrigin {
                                    actor: grant.actor.clone(),
                                    session: grant.session.clone(),
                                    run: grant.run.clone(),
                                    generation: grant.generation.clone(),
                                },
                            },
                        )
                        .map_err(runtime_error)?;
                }
                self.finish_allocation(&mut inner, &child, &source)?;
                Ok(lane_summary(&inner.runtime.state().lanes[&child]))
            }
            _ => Err(refusal("fleet-action-not-authorized")),
        }
    }

    /// Page durable saved-review identities in checkpoint-id order, not completion-time order.
    /// Newly inserted earlier IDs require a refresh; selected immutable results never change.
    pub fn saved_reviews(&self, lane: &str, after: Option<&str>) -> Result<Json, Unavailable> {
        super::id_valid(lane).map_err(runtime_error)?;
        if let Some(after) = after {
            super::id_valid(after).map_err(runtime_error)?;
        }
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        if !inner.runtime.state().lanes.contains_key(lane) {
            return Err(refusal("fleet-lane-missing"));
        }
        let entries: Vec<_> = inner
            .runtime
            .state()
            .checkpoints
            .iter()
            .filter(|(_, checkpoint)| {
                checkpoint.lane == lane
                    && checkpoint.review.is_some()
                    && checkpoint
                        .result
                        .as_ref()
                        .is_some_and(|result| result.complete)
            })
            .collect();
        if after.is_some_and(|after| !entries.iter().any(|(id, _)| id.as_str() == after)) {
            return Err(refusal("fleet-review-cursor-invalid"));
        }
        let remaining: Vec<_> = entries
            .iter()
            .filter(|(id, _)| after.is_none_or(|after| id.as_str() > after))
            .collect();
        let rows: Vec<_> = remaining
            .iter()
            .take(50)
            .map(|(id, checkpoint)| {
                let result = checkpoint
                    .result
                    .as_ref()
                    .expect("complete result filtered");
                Json::object([
                    ("checkpoint", Json::text(id.as_str())),
                    ("version", Json::text(result.version.to_string())),
                    (
                        "bundle",
                        Json::text(
                            checkpoint
                                .review
                                .expect("recorded review filtered")
                                .to_string(),
                        ),
                    ),
                    ("run", Json::text(&checkpoint.origin.run)),
                ])
            })
            .collect();
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-saved-reviews/v1")),
            ("objective", Json::text(inner.runtime.objective())),
            ("lane", Json::text(lane)),
            ("revision", Json::Number(inner.runtime.state().revision)),
            ("order", Json::text("checkpoint-id")),
            ("after", after.map_or(Json::Null, Json::text)),
            ("total", Json::Number(entries.len() as u64)),
            (
                "next_after",
                if remaining.len() > 50 {
                    Json::text(remaining[49].0)
                } else {
                    Json::Null
                },
            ),
            ("reviews", Json::Array(rows)),
        ]))
    }

    /// Verify the immutable review for one saved checkpoint without navigating or reading live files.
    pub fn saved_review(&self, selection: &SavedReviewSelection) -> Result<Json, Unavailable> {
        let review = self.with_saved_review(selection, |workspace| {
            workspace.daemon().recorded_lane_review(
                workspace.binding().root(),
                workspace.binding().installation(),
                selection.bundle,
                selection.version,
            )
        })?;
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-saved-review/v1")),
            ("objective", Json::text(self.objective()?)),
            ("selection", selection.to_json()),
            ("review", review),
        ]))
    }

    /// Read an exact historical artifact; object identity and side select content, never a path.
    pub fn saved_review_artifact(
        &self,
        selection: &SavedReviewSelection,
        object: &str,
        side: &str,
    ) -> Result<crate::ReviewArtifact, Unavailable> {
        self.with_saved_review(selection, |workspace| {
            workspace.daemon().recorded_lane_artifact(
                workspace.binding().root(),
                workspace.binding().installation(),
                selection.bundle,
                selection.version,
                object,
                side,
            )
        })
    }

    fn with_saved_review<T>(
        &self,
        selection: &SavedReviewSelection,
        read: impl FnOnce(&LaneWorkspace) -> Result<T, Unavailable>,
    ) -> Result<T, Unavailable> {
        let workspace = {
            let mut inner = self.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            saved_review_workspace(&inner, selection)?
        };
        // Do not hold the fleet-wide lock while reconstructing artifacts; other lanes keep working.
        let result = read(&workspace)?;
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        let current = saved_review_workspace(&inner, selection)?;
        if !Arc::ptr_eq(&workspace, &current) {
            return Err(refusal("fleet-review-workspace-changed"));
        }
        Ok(result)
    }

    /// Refresh durable native lifecycle state independently of the desktop selection.
    pub fn native_state(&self) -> Result<super::State, Unavailable> {
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        Ok(inner.runtime.state().clone())
    }

    pub(super) fn record_provider_completion(
        &self,
        lane: &str,
        run: &str,
        success: bool,
    ) -> Result<bool, Unavailable> {
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        let current = inner
            .runtime
            .state()
            .lanes
            .get(lane)
            .and_then(|lane| lane.runs.last())
            .ok_or_else(|| refusal("fleet-run-not-active"))?;
        if current.id != run {
            return Err(refusal("stale-run"));
        }
        // Direct process exit is insufficient evidence to finish a cancelled process tree.
        if inner.runtime.state().cancelled || current.state == RunState::Stopping {
            return Ok(false);
        }
        inner
            .runtime
            .record(
                &format!("complete-{run}"),
                Command::Observe {
                    lane: lane.into(),
                    run: run.into(),
                    state: if success {
                        RunState::Succeeded
                    } else {
                        RunState::Failed
                    },
                },
            )
            .map_err(runtime_error)?;
        Ok(true)
    }

    /// Native fleet projection, including all lanes. This is not available through agent scope.
    pub fn snapshot(&self) -> Result<Json, Unavailable> {
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        Ok(Json::object([
            ("objective", Json::text(inner.runtime.objective())),
            ("revision", Json::Number(inner.runtime.state().revision)),
            ("cancelled", Json::Bool(inner.runtime.state().cancelled)),
            (
                "lanes",
                Json::Array(
                    inner
                        .runtime
                        .state()
                        .lanes
                        .values()
                        .map(lane_summary)
                        .collect(),
                ),
            ),
        ]))
    }
}

fn saved_review_workspace(
    inner: &Inner,
    selection: &SavedReviewSelection,
) -> Result<Arc<LaneWorkspace>, Unavailable> {
    let checkpoint = inner
        .runtime
        .state()
        .checkpoints
        .get(&selection.checkpoint)
        .filter(|checkpoint| {
            checkpoint.lane == selection.lane
                && checkpoint.review == Some(selection.bundle)
                && checkpoint
                    .result
                    .as_ref()
                    .is_some_and(|result| result.complete && result.version == selection.version)
        })
        .ok_or_else(|| refusal("fleet-review-selection-mismatch"))?;
    let workspace = inner
        .workspaces
        .get(&checkpoint.lane)
        .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))?;
    if inner
        .runtime
        .state()
        .lanes
        .get(&checkpoint.lane)
        .and_then(|lane| lane.workspace.as_ref())
        != Some(workspace.binding())
    {
        return Err(refusal("fleet-review-workspace-changed"));
    }
    Ok(workspace.clone())
}

fn review_summary(checkpoint: &str, version: RecordDigest, bundle: RecordDigest) -> Json {
    Json::object([
        ("checkpoint", Json::text(checkpoint)),
        ("version", Json::text(version.to_string())),
        ("bundle", Json::text(bundle.to_string())),
        ("recorded", Json::Bool(true)),
    ])
}

fn checkpoint_summary(id: &str, result: &super::CheckpointResult) -> Json {
    Json::object([
        ("checkpoint", Json::text(id)),
        ("complete", Json::Bool(result.complete)),
        ("version", Json::text(result.version.to_string())),
        (
            "workspace_digest",
            Json::text(result.workspace_digest.to_string()),
        ),
        ("saved_changes", Json::Number(result.saved_changes)),
        (
            "issue",
            result.issue.as_ref().map(Json::text).unwrap_or(Json::Null),
        ),
    ])
}

fn exact_state(workspace: &LaneWorkspace) -> Result<WorkspaceSummary, Unavailable> {
    let state = workspace.state()?;
    if state.root != workspace.binding().root()
        || state.installation != workspace.binding().installation()
    {
        return Err(refusal("fleet-lane-identity-changed"));
    }
    Ok(state)
}
fn verify_custody(
    workspace: &LaneWorkspace,
    state: &WorkspaceSummary,
    generation: &str,
) -> Result<(), Unavailable> {
    let custody = workspace
        .daemon()
        .workspace_agent_custody_for_workspace(&state.root, &state.digest, &state.installation)
        .map_err(|_| refusal("fleet-session-custody-changed"))?;
    if custody.generation() != Some(generation) {
        return Err(refusal("fleet-session-custody-changed"));
    }
    Ok(())
}
fn ensure_run(inner: &Inner, lane: &str, run: &str) -> Result<(), Unavailable> {
    if inner.runtime.state().cancelled {
        return Err(refusal("fleet-objective-cancelled"));
    }
    let current = inner
        .runtime
        .state()
        .lanes
        .get(lane)
        .and_then(|lane| lane.runs.last())
        .ok_or_else(|| refusal("fleet-run-not-active"))?;
    if current.id != run
        || !matches!(
            current.state,
            RunState::Launching | RunState::Running | RunState::Waiting
        )
    {
        return Err(refusal("fleet-run-not-active"));
    }
    Ok(())
}
fn token_key(token: &str) -> String {
    Blake3::digest_bytes(token.as_bytes()).to_hex()
}
fn lane_identity(objective: &str, parent: &str, request: &str) -> Result<String, Unavailable> {
    super::id_valid(request).map_err(runtime_error)?;
    let framed = Json::object([
        ("domain", Json::text("mesh.fleet.lane/v1")),
        ("objective", Json::text(objective)),
        ("parent", Json::text(parent)),
        ("request", Json::text(request)),
    ])
    .encode();
    Ok(format!(
        "lane-{}",
        Blake3::digest_bytes(framed.as_bytes()).to_hex()
    ))
}
fn lane_summary(lane: &Lane) -> Json {
    Json::object([
        ("id", Json::text(&lane.id)),
        (
            "parent",
            lane.parent.as_ref().map(Json::text).unwrap_or(Json::Null),
        ),
        (
            "source_project",
            lane.source_project
                .as_ref()
                .map(Json::text)
                .unwrap_or(Json::Null),
        ),
        ("goal", Json::text(&lane.goal)),
        ("provider", Json::text(&lane.provider)),
        ("base", Json::text(lane.base.to_string())),
        ("allocated", Json::Bool(lane.workspace.is_some())),
        (
            "workspace",
            lane.workspace
                .as_ref()
                .map(|w| {
                    Json::object([
                        ("root", Json::text(w.root())),
                        ("installation", Json::text(w.installation())),
                    ])
                })
                .unwrap_or(Json::Null),
        ),
        (
            "run",
            lane.runs
                .last()
                .map(|run| {
                    Json::object([
                        ("id", Json::text(&run.id)),
                        ("state", Json::text(super::wire::state_word(run.state))),
                    ])
                })
                .unwrap_or(Json::Null),
        ),
    ])
}
fn exact_fields(arguments: &Json, expected: &[&str]) -> Result<(), Unavailable> {
    let Json::Object(fields) = arguments else {
        return Err(refusal("fleet-arguments-invalid"));
    };
    if fields.len() != expected.len()
        || fields
            .iter()
            .any(|(key, _)| !expected.contains(&key.as_str()))
    {
        return Err(refusal("fleet-arguments-invalid"));
    }
    Ok(())
}
fn field<'a>(arguments: &'a Json, name: &str) -> Result<&'a str, Unavailable> {
    arguments
        .get(name)
        .and_then(Json::as_text)
        .ok_or_else(|| refusal("fleet-arguments-invalid"))
}
fn runtime_error(error: super::Error) -> Unavailable {
    match error {
        super::Error::Refused(code) => refusal(code),
        super::Error::Store(mesh_store::fleet::FleetStoreError::RequestConflict) => {
            refusal("fleet-request-conflict")
        }
        super::Error::Store(mesh_store::fleet::FleetStoreError::StaleRevision { .. }) => {
            refusal("fleet-state-changed")
        }
        _ => refusal("fleet-history-unavailable"),
    }
}
fn refusal(code: &str) -> Unavailable {
    Unavailable::new(code, "Mesh could not authorize or complete this fleet action. Refresh the fleet and inspect the lane state.")
}
