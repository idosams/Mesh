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

use super::workspace::{LaneHistory, LaneWorkspace, VersionInput};
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
    /// Open verified retained history without attaching an execution context.
    fn reopen_history(
        &self,
        _lane: &str,
        _binding: &super::WorkspaceBinding,
    ) -> Result<LaneHistory, Unavailable> {
        Err(refusal("fleet-history-reopen-unsupported"))
    }
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
    fn reopen_history(
        &self,
        lane: &str,
        binding: &super::WorkspaceBinding,
    ) -> Result<LaneHistory, Unavailable> {
        if !lane.starts_with("lane-")
            || lane.len() != 69
            || !lane[5..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(refusal("fleet-allocation-identity"));
        }
        self.root
            .ensure_namespace_identity()
            .map_err(|_| refusal("fleet-root-unavailable"))?;
        let child = self
            .root
            .open_child_directory(std::ffi::OsStr::new(lane))
            .map_err(|_| refusal("fleet-lane-unavailable"))?;
        let path = Path::new(binding.root());
        let (device, inode) = child
            .identity()
            .map_err(|_| refusal("fleet-lane-unavailable"))?;
        let parent =
            ProtectedWorkspaceRoot::from_directory_token(&format!("{device:016x}:{inode:016x}"))
                .map_err(|_| refusal("fleet-lane-unavailable"))?;
        let open = crate::workspace::OpenWorkspace::reopen_history(
            path,
            binding.installation(),
            parent,
            &self.reviewers,
        )
        .map_err(|_| refusal("fleet-history-unavailable"))?;
        let history = LaneHistory {
            open,
            parents: vec![self.root.clone(), child],
            allocation: parent,
        };
        history.verify()?;
        Ok(history)
    }

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

/// Retained-result access and explicit source-project import/review, without worker adoption.
#[derive(Clone)]
pub struct FleetHistory(pub(crate) Arc<FleetService>);
impl FleetHistory {
    /// Read a fixed whole-project candidate comparison without acquiring publication authority.
    pub fn review_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        page: (Option<&str>, Option<&str>),
    ) -> Result<Json, Unavailable> {
        self.0
            .review_project_candidate(selection, source, trusted, request, expected_main, page)
    }
    /// Read retained import identity and outcome without loading a private signing key.
    pub fn recorded_project_candidate_import(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
    ) -> Result<Option<(mesh_types::PublicKey, Json)>, Unavailable> {
        self.0
            .recorded_project_candidate_import(selection, source, trusted, request, expected_main)
    }
    /// Explicit source-project authoring with a native signer; never adopts fleet execution.
    pub fn import_saved_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        signer: &dyn super::CandidateImportSigner,
    ) -> Result<Json, Unavailable> {
        self.0
            .import_project_candidate(selection, source, trusted, request, expected_main, signer)
    }
    /// Explicit source-project review creation, or read-only recovery when create is false.
    pub fn review_imported_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        create: bool,
    ) -> Result<Json, Unavailable> {
        self.0.review_imported_project_candidate(
            selection,
            source,
            trusted,
            request,
            expected_main,
            create,
        )
    }

    /// Recover exact import status from retained history without adopting execution or signing.
    pub fn inspect_project_candidate_import(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        actor: mesh_types::PublicKey,
    ) -> Result<Json, Unavailable> {
        self.0.inspect_project_candidate_import(
            selection,
            source,
            trusted,
            request,
            expected_main,
            actor,
        )
    }

    /// Recover an existing exact staged candidate without creating content or adopting workers.
    pub fn inspect_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.0
            .inspect_project_candidate(selection, source, trusted, request, expected_main)
    }
    /// Verify a result's original project correspondence through its input ancestry without execution adoption.
    pub fn saved_project_mapping(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        after: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.0
            .saved_project_mapping(selection, source, trusted, after)
    }
    /// List durable saved-result identities.
    pub fn saved_reviews(&self, lane: &str, after: Option<&str>) -> Result<Json, Unavailable> {
        self.0.saved_reviews(lane, after)
    }
    /// Inspect one exact recorded result.
    pub fn saved_review(&self, selection: &SavedReviewSelection) -> Result<Json, Unavailable> {
        self.0.saved_review(selection)
    }
    /// Read exact saved feedback without adopting execution authority.
    pub fn saved_review_changes(
        &self,
        selection: &SavedReviewSelection,
    ) -> Result<Json, Unavailable> {
        self.0.saved_review_changes(selection)
    }
    /// Read exact requests and result proposals without acquiring execution ownership.
    pub fn saved_review_change_activity(
        &self,
        selection: &SavedReviewSelection,
    ) -> Result<Json, Unavailable> {
        self.0.saved_review_change_activity(selection)
    }
    /// Compare a result with its verified local starting version.
    pub fn saved_starting_comparison(
        &self,
        selection: &SavedReviewSelection,
        after: Option<&str>,
        selected: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.0.saved_starting_comparison(selection, after, selected)
    }
    /// Read verified historical bytes for a selected artifact side.
    pub fn saved_review_artifact(
        &self,
        selection: &SavedReviewSelection,
        object: &str,
        side: &str,
    ) -> Result<crate::ReviewArtifact, Unavailable> {
        self.0.saved_review_artifact(selection, object, side)
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
            "missing_files" => {
                exact_fields(arguments, &[])?;
                let inventory = workspace
                    .daemon()
                    .inspect_agent_finish_preflight(
                        &state.root,
                        &state.digest,
                        &state.installation,
                        &grant.generation,
                    )
                    .map_err(|_| refusal("fleet-deletion-inspection-unavailable"))?;
                Ok(Json::object([
                    ("schema", Json::text("mesh.fleet-missing-files/v1")),
                    (
                        "files",
                        Json::Array(
                            inventory
                                .missing_files()
                                .iter()
                                .take(128)
                                .map(|file| {
                                    Json::object([
                                        ("path", Json::text(file.path())),
                                        ("version", Json::text(file.current_version())),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    (
                        "not_listed",
                        Json::Number(inventory.missing_files().len().saturating_sub(128) as u64),
                    ),
                    ("approval_authority", Json::Bool(false)),
                ]))
            }
            "resolve_file_deletion" => {
                exact_fields(arguments, &["request", "path", "version"])?;
                let request = field(arguments, "request")?;
                let path = field(arguments, "path")?;
                let version = RecordDigest::parse_hex(field(arguments, "version")?)
                    .map_err(|_| refusal("fleet-deletion-version-invalid"))?;
                let id = format!(
                    "file-deletion-{}",
                    &lane_identity(inner.runtime.objective(), &grant.lane, request)?[5..]
                );
                let origin = super::AgentOrigin {
                    actor: grant.actor.clone(),
                    session: grant.session.clone(),
                    run: grant.run.clone(),
                    generation: grant.generation.clone(),
                };
                let signer = grant
                    .signer
                    .as_ref()
                    .ok_or_else(|| refusal("fleet-checkpoint-signer-unavailable"))?;
                let public = signer.public_key();
                if RecordDigest::from_bytes(*public.as_bytes()).to_string() != grant.actor {
                    return Err(refusal("fleet-checkpoint-signer-changed"));
                }
                if let Some(previous) = inner.runtime.state().file_deletions.get(&id) {
                    if previous.lane != grant.lane
                        || previous.origin != origin
                        || previous.path != path
                        || previous.version != version
                    {
                        return Err(refusal("fleet-deletion-request-conflict"));
                    }
                    if let Some(result) = &previous.result {
                        return Ok(file_deletion_summary(&id, result));
                    }
                    if let Some(operation) = previous.operation {
                        if workspace
                            .daemon()
                            .inspect_agent_prepared_operation(
                                crate::AgentWorkspaceCheckpointRequest {
                                    root: &state.root,
                                    digest: &state.digest,
                                    installation: &state.installation,
                                    generation: &grant.generation,
                                },
                                operation,
                                public,
                            )
                            .map_err(|_| refusal("fleet-deletion-proof-unavailable"))?
                        {
                            let result = super::FileDeletionResult {
                                operation,
                                workspace_digest: state.digest.clone(),
                                settled: false,
                            };
                            inner
                                .runtime
                                .record(
                                    &format!("finish-{id}"),
                                    Command::FinishFileDeletion {
                                        id: id.clone(),
                                        result: result.clone(),
                                    },
                                )
                                .map_err(runtime_error)?;
                            return Ok(file_deletion_summary(&id, &result));
                        }
                    }
                    if previous.input_digest != state.digest {
                        return Err(refusal("fleet-deletion-needs-reconciliation"));
                    }
                } else {
                    let inventory = workspace
                        .daemon()
                        .inspect_agent_finish_preflight(
                            &state.root,
                            &state.digest,
                            &state.installation,
                            &grant.generation,
                        )
                        .map_err(|_| refusal("fleet-deletion-inspection-unavailable"))?;
                    if !inventory.missing_files().iter().any(|file| {
                        file.path() == path && file.current_version() == version.to_string()
                    }) {
                        return Err(refusal("fleet-deletion-inspection-changed"));
                    }
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
                            Command::BeginFileDeletion {
                                id: id.clone(),
                                lane: grant.lane.clone(),
                                origin,
                                input_digest: state.digest.clone(),
                                path: path.into(),
                                version,
                            },
                        )
                        .map_err(runtime_error)?;
                }
                let prepared = inner.runtime.state().file_deletions[&id].operation;
                let receipt = workspace
                    .daemon()
                    .checkpoint_agent_file_deletion_prepared(
                        crate::AgentWorkspaceCheckpointRequest {
                            root: &state.root,
                            digest: &state.digest,
                            installation: &state.installation,
                            generation: &grant.generation,
                        },
                        path,
                        &version.to_string(),
                        public,
                        |payload| signer.sign(payload),
                        |operation| {
                            if prepared.is_some_and(|expected| expected != operation) {
                                return Err("prepared deletion operation changed".to_owned());
                            }
                            inner
                                .runtime
                                .record(
                                    &format!("prepare-{id}"),
                                    Command::PrepareFileDeletion {
                                        id: id.clone(),
                                        operation,
                                    },
                                )
                                .map(|_| ())
                                .map_err(|e| e.to_string())
                        },
                    )
                    .map_err(|_| refusal("fleet-deletion-needs-reconciliation"))?;
                let result = super::FileDeletionResult {
                    operation: RecordDigest::parse_hex(receipt.changeset())
                        .map_err(|_| refusal("fleet-deletion-operation-invalid"))?,
                    workspace_digest: exact_state(&workspace)?.digest,
                    settled: receipt.meaningful_saved(),
                };
                inner
                    .runtime
                    .record(
                        &format!("finish-{id}"),
                        Command::FinishFileDeletion {
                            id: id.clone(),
                            result: result.clone(),
                        },
                    )
                    .map_err(runtime_error)?;
                Ok(file_deletion_summary(&id, &result))
            }
            "propose_review_change_result" => {
                exact_fields(arguments, &["request", "checkpoint"])?;
                let request = field(arguments, "request")?;
                let checkpoint = field(arguments, "checkpoint")?;
                let feedback = inner
                    .runtime
                    .state()
                    .review_change_requests
                    .get(request)
                    .ok_or_else(|| refusal("fleet-review-change-request-unavailable"))?;
                if feedback.lane != grant.lane {
                    return Err(refusal("fleet-review-change-not-in-lane"));
                }
                let origin = super::AgentOrigin {
                    actor: grant.actor.clone(),
                    session: grant.session.clone(),
                    run: grant.run.clone(),
                    generation: grant.generation.clone(),
                };
                let saved = inner
                    .runtime
                    .state()
                    .checkpoints
                    .get(checkpoint)
                    .filter(|saved| saved.lane == grant.lane && saved.origin == origin)
                    .ok_or_else(|| refusal("fleet-checkpoint-not-in-session"))?;
                let result = saved
                    .result
                    .as_ref()
                    .filter(|result| result.complete)
                    .ok_or_else(|| refusal("fleet-checkpoint-incomplete"))?;
                let bundle = saved
                    .review
                    .ok_or_else(|| refusal("fleet-review-not-recorded"))?;
                // Verify the recorded immutable result while preserving the current workspace and
                // native custody. A checkpoint identity alone is not proof of retained review bytes.
                workspace.daemon().with_recorded_lane_review(
                    &state.root,
                    &state.installation,
                    bundle,
                    result.version,
                    |_open| Ok(()),
                )?;
                let framed = Json::object([
                    ("request", Json::text(request)),
                    ("checkpoint", Json::text(checkpoint)),
                ])
                .encode();
                let id = format!("review-response-{}", token_key(&framed));
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
                        &id,
                        Command::ProposeReviewChangeResult {
                            request: request.into(),
                            checkpoint: checkpoint.into(),
                            origin,
                        },
                    )
                    .map_err(runtime_error)?;
                let response = inner
                    .runtime
                    .state()
                    .review_change_responses
                    .get(request)
                    .and_then(|responses| {
                        responses
                            .iter()
                            .find(|response| response.checkpoint == checkpoint)
                    })
                    .ok_or_else(|| refusal("fleet-review-response-unavailable"))?;
                Ok(review_change_response_json(&grant.lane, response))
            }
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
                    (
                        "review_change_decisions",
                        Json::Array(review_change_decision_rows(
                            inner.runtime.state(),
                            &grant.lane,
                            None,
                        )),
                    ),
                    (
                        "review_change_responses",
                        Json::Array(review_change_response_rows(
                            inner.runtime.state(),
                            &grant.lane,
                            None,
                        )),
                    ),
                    (
                        "review_change_requests",
                        Json::Array(
                            inner
                                .runtime
                                .state()
                                .review_change_requests
                                .values()
                                .filter(|request| request.lane == grant.lane)
                                .map(review_change_json)
                                .collect(),
                        ),
                    ),
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

    /// Record native-requested feedback against authenticated saved history. Agents cannot call
    /// this method through their scoped action router. No worker is launched or marked notified.
    pub fn request_review_changes(
        &self,
        request: &str,
        selection: &SavedReviewSelection,
        message: &str,
    ) -> Result<Json, Unavailable> {
        super::id_valid(request).map_err(runtime_error)?;
        super::review_change_message_valid(message).map_err(runtime_error)?;
        self.with_saved_review(selection, |_open, _binding| Ok(()))?;
        let id = format!("review-change-{}", token_key(request));
        let changes = super::ReviewChangeRequest {
            id: id.clone(),
            lane: selection.lane.clone(),
            checkpoint: selection.checkpoint.clone(),
            version: selection.version,
            bundle: selection.bundle,
            message: message.to_owned(),
        };
        let mut inner = self.lock()?;
        inner
            .runtime
            .record(&id, Command::RequestReviewChanges(changes.clone()))
            .map_err(runtime_error)?;
        Ok(review_change_json(&changes))
    }

    /// Native-confirmed, reversible work decision. Confirmation runs without the fleet lock;
    /// retained identities and the exact per-request revision are checked again before append.
    pub fn decide_review_change(
        &self,
        selection: &SavedReviewSelection,
        request: &str,
        operation: &str,
        expected_revision: u64,
        checkpoint: Option<&str>,
        confirm: impl FnOnce(&str) -> bool,
    ) -> Result<Json, Unavailable> {
        super::id_valid(operation).map_err(runtime_error)?;
        if expected_revision >= 64 {
            return Err(refusal("fleet-review-decision-limit"));
        }
        let command = Command::SetReviewChangeDecision {
            request: request.into(),
            expected_revision,
            checkpoint: checkpoint.map(str::to_owned),
        };
        let id = format!("review-decision-{}", token_key(operation));
        let (message, proposed) = {
            let mut inner = self.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            let feedback = inner
                .runtime
                .state()
                .review_change_requests
                .get(request)
                .filter(|feedback| {
                    feedback.lane == selection.lane
                        && feedback.checkpoint == selection.checkpoint
                        && feedback.version == selection.version
                        && feedback.bundle == selection.bundle
                })
                .cloned()
                .ok_or_else(|| refusal("fleet-review-change-selection-mismatch"))?;
            if let Some(receipt) = inner.runtime.recorded(&id).map_err(runtime_error)? {
                if receipt.payload != super::wire::encode(&command) {
                    return Err(refusal("fleet-review-decision-operation-conflict"));
                }
                return Ok(review_decision_outcome(
                    inner.runtime.state(),
                    request,
                    false,
                    Some(expected_revision + 1),
                    checkpoint,
                ));
            }
            let before = inner
                .runtime
                .state()
                .review_change_decisions
                .get(request)
                .cloned()
                .unwrap_or_default();
            if before.revision != expected_revision
                || before.revision >= 64
                || before.checkpoint.as_deref() == checkpoint
            {
                return Err(refusal("fleet-review-decision-stale-or-unchanged"));
            }
            let proposed = checkpoint
                .map(|checkpoint| {
                    let response = inner
                        .runtime
                        .state()
                        .review_change_responses
                        .get(request)
                        .and_then(|responses| {
                            responses
                                .iter()
                                .find(|response| response.checkpoint == checkpoint)
                        })
                        .ok_or_else(|| refusal("fleet-review-proposal-unavailable"))?;
                    SavedReviewSelection::new(
                        &selection.lane,
                        checkpoint,
                        &response.version.to_string(),
                        &response.bundle.to_string(),
                    )
                })
                .transpose()?;
            (feedback.message, proposed)
        };
        self.with_saved_review(selection, |_open, _binding| Ok(()))?;
        if let Some(proposed) = &proposed {
            self.with_saved_review(proposed, |_open, _binding| Ok(()))?;
        }
        let action = proposed.as_ref().map_or_else(
            || "Reopen this change request".to_owned(),
            |proposed| {
                format!(
                    "Mark this change request addressed by saved version {}\nReview: {}",
                    proposed.version, proposed.bundle
                )
            },
        );
        let prompt = format!("{action}\n\nRequest: {request}\nLane: {}\nOriginal saved version: {}\nOriginal review: {}\nDecision revision: {expected_revision}\n\nRequested changes:\n{message}\n\nThis changes only the request's work status. It does not approve, publish, apply files, or start an agent.", selection.lane, selection.version, selection.bundle);
        if !confirm(&prompt) {
            let mut inner = self.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            return Ok(review_decision_outcome(
                inner.runtime.state(),
                request,
                true,
                None,
                None,
            ));
        }
        self.with_saved_review(selection, |_open, _binding| Ok(()))?;
        if let Some(proposed) = &proposed {
            self.with_saved_review(proposed, |_open, _binding| Ok(()))?;
        }
        let mut inner = self.lock()?;
        inner.runtime.record(&id, command).map_err(runtime_error)?;
        Ok(review_decision_outcome(
            inner.runtime.state(),
            request,
            false,
            Some(expected_revision + 1),
            checkpoint,
        ))
    }

    /// Read recorded feedback for an exact saved selection, without worker or approval authority.
    pub fn saved_review_changes(
        &self,
        selection: &SavedReviewSelection,
    ) -> Result<Json, Unavailable> {
        self.with_saved_review(selection, |_open, _binding| Ok(()))?;
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        Ok(Json::Array(
            inner
                .runtime
                .state()
                .review_change_requests
                .values()
                .filter(|request| {
                    request.lane == selection.lane
                        && request.checkpoint == selection.checkpoint
                        && request.version == selection.version
                        && request.bundle == selection.bundle
                })
                .map(review_change_json)
                .collect(),
        ))
    }

    /// Read requests and proposed results together at one observed runtime revision.
    pub fn saved_review_change_activity(
        &self,
        selection: &SavedReviewSelection,
    ) -> Result<Json, Unavailable> {
        self.with_saved_review(selection, |_open, _binding| Ok(()))?;
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        let state = inner.runtime.state();
        Ok(Json::object([
            (
                "decisions",
                Json::Array(review_change_decision_rows(
                    state,
                    &selection.lane,
                    Some(selection),
                )),
            ),
            (
                "changes",
                Json::Array(
                    state
                        .review_change_requests
                        .values()
                        .filter(|request| {
                            request.lane == selection.lane
                                && request.checkpoint == selection.checkpoint
                                && request.version == selection.version
                                && request.bundle == selection.bundle
                        })
                        .map(review_change_json)
                        .collect(),
                ),
            ),
            (
                "responses",
                Json::Array(review_change_response_rows(
                    state,
                    &selection.lane,
                    Some(selection),
                )),
            ),
        ]))
    }

    /// Verify the immutable review for one saved checkpoint without navigating or reading live files.
    pub fn saved_review(&self, selection: &SavedReviewSelection) -> Result<Json, Unavailable> {
        let review = self.with_saved_review(selection, |open, _binding| {
            open.recorded_review_item(selection.bundle)
                .ok_or_else(|| refusal("fleet-review-unavailable"))
        })?;
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-saved-review/v1")),
            ("objective", Json::text(self.objective()?)),
            ("selection", selection.to_json()),
            ("review", review),
        ]))
    }

    /// Compare a recorded result to the exact local copy of its original lane input.
    /// This read never creates a publication bundle or advances main. Cursors select changed objects.
    pub fn saved_starting_comparison(
        &self,
        selection: &SavedReviewSelection,
        after: Option<&str>,
        selected: Option<&str>,
    ) -> Result<Json, Unavailable> {
        let comparison = self.with_saved_review(selection, |open, binding| {
            let comparison = super::comparison::compare(
                open,
                binding
                    .starting_version()
                    .ok_or_else(|| refusal("fleet-starting-version-unbound"))?,
                selection.version,
                after,
                selected,
            )?;
            Ok(Json::object([
                (
                    "source_version",
                    Json::text(binding.source_version.to_string()),
                ),
                ("comparison", comparison),
            ]))
        })?;
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-starting-comparison/v1")),
            ("objective", Json::text(self.objective()?)),
            ("selection", selection.to_json()),
            ("input", comparison),
            ("approval_authority", Json::Bool(false)),
        ]))
    }

    /// Verify every recorded input boundary from the original project through a delegated result.
    /// All ancestry changes remain visible. This read neither approves dependencies nor reserves main.
    pub fn saved_project_mapping(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        after: Option<&str>,
    ) -> Result<Json, Unavailable> {
        let state = self.native_state()?;
        saved_review_binding_from_state(&state, selection)?;
        let lineage = super::project_mapping::lineage(
            &state,
            &selection.lane,
            selection.version,
            source.id(),
        )
        .map_err(|_| refusal("fleet-project-lineage-unavailable"))?;
        // Keep every retained history guard until the entire chain has been read and revalidated.
        // Opening these readers does not adopt live worker handles or hold their daemon locks.
        let histories = lineage
            .iter()
            .map(|step| self.allocator.reopen_history(&step.lane, &step.binding))
            .collect::<Result<Vec<_>, _>>()?;
        let leaf = histories
            .last()
            .ok_or_else(|| refusal("fleet-project-lineage-unavailable"))?;
        leaf.open
            .review(&selection.bundle)
            .filter(|review| review.subject_operation == selection.version)
            .ok_or_else(|| refusal("fleet-review-not-recorded"))?;
        let root = &lineage[0];
        let mapped = source
            .with_fleet_input(root.binding.source_version, trusted, |project, main| {
                let preview = |open: &crate::workspace::OpenWorkspace, version| {
                    open.historical_workspace_preview(version)
                        .map_err(|error| std::io::Error::other(error.to_string()))
                };
                let mut snapshots = Vec::new();
                let mut inputs = Vec::new();
                for (step, history) in lineage.iter().zip(&histories) {
                    history
                        .verify()
                        .map_err(|_| std::io::Error::other("lineage history changed"))?;
                    let starting = step
                        .binding
                        .starting_version()
                        .ok_or_else(|| std::io::Error::other("unbound input"))?;
                    snapshots.push((
                        preview(&history.open, starting)?,
                        preview(&history.open, step.result)?,
                    ));
                    inputs.push(Json::object([
                        ("lane", Json::text(&step.lane)),
                        (
                            "source_version",
                            Json::text(step.binding.source_version.to_string()),
                        ),
                        ("starting_version", Json::text(starting.to_string())),
                        ("result_version", Json::text(step.result.to_string())),
                    ]));
                }
                let correspondence = super::project_mapping::mapping_chain(
                    preview(project, root.binding.source_version)?,
                    snapshots,
                    after,
                )?;
                Ok(Json::object([
                    ("source_project", Json::text(source.id())),
                    (
                        "source_version",
                        Json::text(root.binding.source_version.to_string()),
                    ),
                    ("lineage", Json::Array(inputs)),
                    ("scope", Json::text("recorded-input-ancestry")),
                    ("observed_main", main),
                    ("correspondence", correspondence),
                ]))
            })
            .map_err(|_| refusal("fleet-project-mapping-unavailable"))?;
        for history in &histories {
            history.verify()?;
        }
        let current = self.native_state()?;
        saved_review_binding_from_state(&current, selection)?;
        if super::project_mapping::lineage(
            &current,
            &selection.lane,
            selection.version,
            source.id(),
        )
        .map_err(|_| refusal("fleet-project-lineage-unavailable"))?
            != lineage
        {
            return Err(refusal("fleet-project-lineage-changed"));
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-project-mapping/v2")),
            ("objective", Json::text(self.objective()?)),
            ("selection", selection.to_json()),
            ("mapping", mapped),
            ("approval_authority", Json::Bool(false)),
        ]))
    }

    /// Stage exact candidate content outside the original project's capture history and files.
    /// Native callers choose a stable request and the main head observed during preparation.
    /// This does not create a project review, import operations, approve, or write back files.
    pub fn stage_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.project_candidate(selection, source, trusted, request, expected_main, true)
    }

    /// Inspect only an existing, complete candidate. Missing or partial content is never repaired.
    pub fn inspect_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
    ) -> Result<Json, Unavailable> {
        self.project_candidate(selection, source, trusted, request, expected_main, false)
    }

    fn project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        create: bool,
    ) -> Result<Json, Unavailable> {
        if let Some(head) = expected_main {
            if RecordDigest::parse_hex(head)
                .ok()
                .is_none_or(|parsed| parsed.to_string() != head)
            {
                return Err(refusal("fleet-candidate-main-invalid"));
            }
        }
        let mapping = self.saved_project_mapping(selection, source, trusted, None)?;
        let state = self.native_state()?;
        let binding = saved_review_binding_from_state(&state, selection)?;
        let origin = &state.checkpoints[&selection.checkpoint].origin;
        let objective = self.objective()?;
        let provenance =
            candidate_provenance(&objective, selection, &mapping, origin, expected_main)?;
        let main_matches = |mapping: &Json| {
            mapping
                .get("mapping")
                .and_then(|value| value.get("observed_main"))
                .and_then(|value| value.get("head"))
                .and_then(Json::as_text)
                == expected_main
        };
        let history = self.allocator.reopen_history(&selection.lane, binding)?;
        history.verify()?;
        let snapshot = history
            .open
            .historical_workspace_preview(selection.version)
            .map_err(|_| refusal("fleet-candidate-content-unavailable"))?;
        let result = source
            .stage_fleet_candidate(
                request,
                &provenance,
                &history.open,
                &snapshot,
                if create {
                    crate::project_attachment::CandidateAdmission::Stage {
                        main_matches: main_matches(&mapping),
                    }
                } else {
                    crate::project_attachment::CandidateAdmission::Inspect
                },
                || {
                    history
                        .verify()
                        .map_err(|_| std::io::Error::other("candidate history changed"))?;
                    let current = self
                        .saved_project_mapping(selection, source, trusted, None)
                        .map_err(|_| std::io::Error::other("candidate lineage unavailable"))?;
                    let same = candidate_provenance(
                        &objective,
                        selection,
                        &current,
                        origin,
                        expected_main,
                    )
                    .map_err(|_| std::io::Error::other("candidate provenance unavailable"))?
                        == provenance;
                    if !same || !main_matches(&current) {
                        return Err(std::io::Error::other(
                            "candidate input or main changed during preparation",
                        ));
                    }
                    Ok(())
                },
            )
            .map_err(|_| refusal("fleet-candidate-staging-unavailable"))?;
        history.verify()?;
        Ok(result)
    }

    /// Recover a public import identity only from verified retained proof. No key creation or signing.
    pub fn recorded_project_candidate_import(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
    ) -> Result<Option<(mesh_types::PublicKey, Json)>, Unavailable> {
        let candidate =
            self.inspect_project_candidate(selection, source, trusted, request, expected_main)?;
        let result = self.with_saved_review(selection, |open, _| {
            let snapshot = open
                .historical_workspace_preview(selection.version)
                .map_err(|_| refusal("fleet-candidate-content-unavailable"))?;
            source
                .recorded_fleet_import(request, &candidate, &snapshot, trusted)
                .map_err(|_| refusal("fleet-candidate-import-unavailable"))
        })?;
        if self.inspect_project_candidate(selection, source, trusted, request, expected_main)?
            != candidate
        {
            return Err(refusal("fleet-candidate-import-input-changed"));
        }
        Ok(result)
    }

    /// Open or inspect the actual imported review against the candidate's fixed, verified main base.
    /// An explicit historical review may be stale; it grants no approval or implicit rebase.
    pub fn review_imported_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        create: bool,
    ) -> Result<Json, Unavailable> {
        let candidate =
            self.inspect_project_candidate(selection, source, trusted, request, expected_main)?;
        let result = self.with_saved_review(selection, |open, _| {
            let snapshot = open
                .historical_workspace_preview(selection.version)
                .map_err(|_| refusal("fleet-candidate-content-unavailable"))?;
            source
                .review_fleet_import(request, &candidate, &snapshot, trusted, create)
                .map_err(|_| refusal("fleet-candidate-import-review-unavailable"))
        })?;
        if self.inspect_project_candidate(selection, source, trusted, request, expected_main)?
            != candidate
        {
            return Err(refusal("fleet-candidate-import-input-changed"));
        }
        Ok(result)
    }

    /// Inspect only durable import intent and journal completion. This never signs or appends.
    pub fn inspect_project_candidate_import(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        actor: mesh_types::PublicKey,
    ) -> Result<Json, Unavailable> {
        let candidate =
            self.inspect_project_candidate(selection, source, trusted, request, expected_main)?;
        let result = self
            .with_saved_review(selection, |open, _| {
                let snapshot = open
                    .historical_workspace_preview(selection.version)
                    .map_err(|_| refusal("fleet-candidate-content-unavailable"))?;
                source
                    .inspect_fleet_import(request, &candidate, &snapshot, actor, trusted)
                    .map_err(|_| refusal("fleet-candidate-import-unavailable"))
            })?
            .unwrap_or(Json::Null);
        if self.inspect_project_candidate(selection, source, trusted, request, expected_main)?
            != candidate
        {
            return Err(refusal("fleet-candidate-import-input-changed"));
        }
        Ok(result)
    }

    /// Append a provenance-bound private project version. Exact retries recover journal truth;
    /// this never creates approval, advances main or writes the original project folder.
    pub fn import_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        signer: &dyn super::CandidateImportSigner,
    ) -> Result<Json, Unavailable> {
        let existing = self.inspect_project_candidate_import(
            selection,
            source,
            trusted,
            request,
            expected_main,
            signer.public_key(),
        )?;
        if existing.get("state") == Some(&Json::text("imported")) {
            return Ok(existing);
        }
        let plan = self.prepare_project_candidate_import(
            selection,
            source,
            trusted,
            request,
            expected_main,
            signer.public_key(),
        )?;
        let candidate =
            self.inspect_project_candidate(selection, source, trusted, request, expected_main)?;
        let result = source
            .commit_fleet_import(request, &candidate, plan, signer, trusted)
            .map_err(|_| refusal("fleet-candidate-import-unavailable"))?;
        if self.inspect_project_candidate(selection, source, trusted, request, expected_main)?
            != candidate
        {
            return Err(refusal("fleet-candidate-import-input-changed"));
        }
        Ok(result)
    }

    /// Compile a staged result using complete native ancestry correspondence. This is read-only:
    /// it does not append, sign, create a project review, advance main or touch ordinary files.
    pub fn prepare_project_candidate_import(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        actor: mesh_types::PublicKey,
    ) -> Result<super::PreparedProjectCandidateImport, Unavailable> {
        let candidate =
            self.inspect_project_candidate(selection, source, trusted, request, expected_main)?;
        let state = self.native_state()?;
        saved_review_binding_from_state(&state, selection)?;
        let lineage = super::project_mapping::lineage(
            &state,
            &selection.lane,
            selection.version,
            source.id(),
        )
        .map_err(|_| refusal("fleet-project-lineage-unavailable"))?;
        let histories = lineage
            .iter()
            .map(|step| self.allocator.reopen_history(&step.lane, &step.binding))
            .collect::<Result<Vec<_>, _>>()?;
        let leaf = histories
            .last()
            .ok_or_else(|| refusal("fleet-project-lineage-unavailable"))?;
        let root = &lineage[0];
        let prepared = source
            .with_fleet_input(root.binding.source_version, trusted, |project, main| {
                if main.get("head").and_then(Json::as_text) != expected_main {
                    return Err(std::io::Error::other(
                        "candidate main changed before import preparation",
                    ));
                }
                let preview = |open: &crate::workspace::OpenWorkspace, version| {
                    open.historical_workspace_preview(version)
                        .map_err(|error| std::io::Error::other(error.to_string()))
                };
                let mut snapshots = Vec::new();
                for (step, history) in lineage.iter().zip(&histories) {
                    history
                        .verify()
                        .map_err(|_| std::io::Error::other("lineage history changed"))?;
                    let starting = step
                        .binding
                        .starting_version()
                        .ok_or_else(|| std::io::Error::other("unbound input"))?;
                    snapshots.push((
                        preview(&history.open, starting)?,
                        preview(&history.open, step.result)?,
                    ));
                }
                let origins = super::project_mapping::import_correspondence(
                    preview(project, root.binding.source_version)?,
                    snapshots,
                )?;
                super::project_import::compile(
                    project,
                    root.binding.source_version,
                    &leaf.open,
                    &preview(&leaf.open, selection.version)?,
                    &origins,
                    &candidate,
                    actor,
                )
            })
            .map_err(|_| refusal("fleet-candidate-import-plan-unavailable"))?;
        for history in &histories {
            history.verify()?;
        }
        if self.inspect_project_candidate(selection, source, trusted, request, expected_main)?
            != candidate
        {
            return Err(refusal("fleet-candidate-import-input-changed"));
        }
        Ok(prepared)
    }

    /// Read a complete candidate against its recorded original-project main base. The derived
    /// review identity stays fixed across pages, newer captures and main advancement. No approval.
    pub fn review_project_candidate(
        &self,
        selection: &SavedReviewSelection,
        source: &crate::project_attachment::ProvisionedAttachment,
        trusted: &crate::TrustedReviewers,
        request: &str,
        expected_main: Option<&str>,
        page: (Option<&str>, Option<&str>),
    ) -> Result<Json, Unavailable> {
        let candidate =
            self.inspect_project_candidate(selection, source, trusted, request, expected_main)?;
        let state = self.native_state()?;
        let binding = saved_review_binding_from_state(&state, selection)?;
        let history = self.allocator.reopen_history(&selection.lane, binding)?;
        history.verify()?;
        let snapshot = history
            .open
            .historical_workspace_preview(selection.version)
            .map_err(|_| refusal("fleet-candidate-content-unavailable"))?;
        let review = source
            .review_fleet_candidate(&candidate, &history.open, snapshot, trusted, page)
            .map_err(|_| refusal("fleet-candidate-review-unavailable"))?;
        history.verify()?;
        if self.inspect_project_candidate(selection, source, trusted, request, expected_main)?
            != candidate
        {
            return Err(refusal("fleet-candidate-review-changed"));
        }
        Ok(review)
    }

    /// Read an exact historical artifact; object identity and side select content, never a path.
    pub fn saved_review_artifact(
        &self,
        selection: &SavedReviewSelection,
        object: &str,
        side: &str,
    ) -> Result<crate::ReviewArtifact, Unavailable> {
        let object = mesh_materializer::ObjectId::parse(object)
            .map_err(|_| refusal("fleet-review-object-invalid"))?;
        let side = match side {
            "before" => crate::workspace::ReviewArtifactSide::Before,
            "after" => crate::workspace::ReviewArtifactSide::After,
            _ => return Err(refusal("fleet-review-side-invalid")),
        };
        self.with_saved_review(selection, |open, _binding| {
            let artifact = open
                .verified_review_artifact(selection.bundle, selection.version, object, side)
                .map_err(|_| refusal("fleet-review-artifact-unavailable"))?;
            Ok(crate::ReviewArtifact::from_verified(artifact))
        })
    }

    fn with_saved_review<T>(
        &self,
        selection: &SavedReviewSelection,
        read: impl FnOnce(
            &crate::workspace::OpenWorkspace,
            &super::WorkspaceBinding,
        ) -> Result<T, Unavailable>,
    ) -> Result<T, Unavailable> {
        let (binding, workspace) = {
            let mut inner = self.lock()?;
            inner.runtime.refresh().map_err(runtime_error)?;
            let binding = saved_review_binding(&inner, selection)?.clone();
            let workspace = inner.workspaces.get(&selection.lane).cloned();
            if workspace
                .as_ref()
                .is_some_and(|workspace| workspace.binding() != &binding)
            {
                return Err(refusal("fleet-review-workspace-changed"));
            }
            (binding, workspace)
        };
        // History reads do not hold the fleet lock or insert a recovered execution context.
        let result = if let Some(workspace) = &workspace {
            workspace.daemon().with_recorded_lane_review(
                binding.root(),
                binding.installation(),
                selection.bundle,
                selection.version,
                |open| read(open, &binding),
            )?
        } else {
            let history = self.allocator.reopen_history(&selection.lane, &binding)?;
            history.verify()?;
            history
                .open
                .review(&selection.bundle)
                .filter(|review| {
                    review.subject_operation == selection.version
                        && review.bundle == selection.bundle
                })
                .ok_or_else(|| refusal("fleet-review-not-recorded"))?;
            let result = read(&history.open, &binding)?;
            history.verify()?;
            result
        };
        let mut inner = self.lock()?;
        inner.runtime.refresh().map_err(runtime_error)?;
        if saved_review_binding(&inner, selection)? != &binding
            || match (&workspace, inner.workspaces.get(&selection.lane)) {
                (Some(before), Some(after)) => !Arc::ptr_eq(before, after),
                (None, None) => false,
                _ => true,
            }
        {
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

fn saved_review_binding<'a>(
    inner: &'a Inner,
    selection: &SavedReviewSelection,
) -> Result<&'a super::WorkspaceBinding, Unavailable> {
    saved_review_binding_from_state(inner.runtime.state(), selection)
}

fn candidate_provenance(
    objective: &str,
    selection: &SavedReviewSelection,
    mapping: &Json,
    origin: &super::AgentOrigin,
    expected_main: Option<&str>,
) -> Result<Json, Unavailable> {
    let mapping = mapping
        .get("mapping")
        .ok_or_else(|| refusal("fleet-candidate-mapping-unavailable"))?;
    let field = |name| {
        mapping
            .get(name)
            .cloned()
            .ok_or_else(|| refusal("fleet-candidate-mapping-unavailable"))
    };
    Ok(Json::object([
        ("schema", Json::text("mesh.fleet-candidate-provenance/v1")),
        ("objective", Json::text(objective)),
        ("selection", selection.to_json()),
        ("source_project", field("source_project")?),
        ("source_version", field("source_version")?),
        ("lineage", field("lineage")?),
        (
            "expected_main",
            expected_main.map_or(Json::Null, Json::text),
        ),
        (
            "origin",
            Json::object([
                ("actor", Json::text(&origin.actor)),
                ("session", Json::text(&origin.session)),
                ("run", Json::text(&origin.run)),
                ("generation", Json::text(&origin.generation)),
            ]),
        ),
        ("attribution", Json::text("recorded-agent-checkpoint")),
        ("approval_authority", Json::Bool(false)),
    ]))
}

fn saved_review_binding_from_state<'a>(
    state: &'a super::State,
    selection: &SavedReviewSelection,
) -> Result<&'a super::WorkspaceBinding, Unavailable> {
    let checkpoint = state
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
    state
        .lanes
        .get(&checkpoint.lane)
        .and_then(|lane| lane.workspace.as_ref())
        .ok_or_else(|| refusal("fleet-lane-needs-reattachment"))
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
fn review_change_decision_json(
    state: &super::State,
    request: &str,
    revision: u64,
    checkpoint: Option<&str>,
) -> Json {
    let response = checkpoint.and_then(|checkpoint| {
        state
            .review_change_responses
            .get(request)
            .and_then(|responses| {
                responses
                    .iter()
                    .find(|response| response.checkpoint == checkpoint)
            })
    });
    Json::object([
        ("request", Json::text(request)),
        ("revision", Json::Number(revision)),
        (
            "status",
            Json::text(if checkpoint.is_some() {
                "addressed"
            } else {
                "open"
            }),
        ),
        ("checkpoint", checkpoint.map_or(Json::Null, Json::text)),
        (
            "version",
            response.map_or(Json::Null, |response| {
                Json::text(response.version.to_string())
            }),
        ),
        (
            "bundle",
            response.map_or(Json::Null, |response| {
                Json::text(response.bundle.to_string())
            }),
        ),
        ("approval_authority", Json::Bool(false)),
    ])
}
fn review_decision_outcome(
    state: &super::State,
    request: &str,
    cancelled: bool,
    receipt: Option<u64>,
    checkpoint: Option<&str>,
) -> Json {
    let current = state
        .review_change_decisions
        .get(request)
        .cloned()
        .unwrap_or_default();
    Json::object([
        ("cancelled", Json::Bool(cancelled)),
        (
            "receipt",
            receipt.map_or(Json::Null, |revision| {
                review_change_decision_json(state, request, revision, checkpoint)
            }),
        ),
        (
            "current",
            review_change_decision_json(
                state,
                request,
                current.revision,
                current.checkpoint.as_deref(),
            ),
        ),
    ])
}
fn review_change_decision_rows(
    state: &super::State,
    lane: &str,
    selection: Option<&SavedReviewSelection>,
) -> Vec<Json> {
    state
        .review_change_requests
        .values()
        .filter(|request| {
            request.lane == lane
                && selection.is_none_or(|selected| {
                    request.checkpoint == selected.checkpoint
                        && request.version == selected.version
                        && request.bundle == selected.bundle
                })
        })
        .map(|request| {
            let current = state
                .review_change_decisions
                .get(&request.id)
                .cloned()
                .unwrap_or_default();
            review_change_decision_json(
                state,
                &request.id,
                current.revision,
                current.checkpoint.as_deref(),
            )
        })
        .collect()
}
fn review_change_response_rows(
    state: &super::State,
    lane: &str,
    selection: Option<&SavedReviewSelection>,
) -> Vec<Json> {
    state
        .review_change_requests
        .values()
        .filter(|request| {
            request.lane == lane
                && selection.is_none_or(|selected| {
                    request.checkpoint == selected.checkpoint
                        && request.version == selected.version
                        && request.bundle == selected.bundle
                })
        })
        .flat_map(|request| {
            state
                .review_change_responses
                .get(&request.id)
                .into_iter()
                .flatten()
        })
        .map(|response| review_change_response_json(lane, response))
        .collect()
}
fn review_change_response_json(lane: &str, response: &super::ReviewChangeResponse) -> Json {
    Json::object([
        ("request", Json::text(&response.request)),
        ("lane", Json::text(lane)),
        ("checkpoint", Json::text(&response.checkpoint)),
        ("version", Json::text(response.version.to_string())),
        ("bundle", Json::text(response.bundle.to_string())),
        ("status", Json::text("proposed")),
        ("approval_authority", Json::Bool(false)),
    ])
}
fn review_change_json(request: &super::ReviewChangeRequest) -> Json {
    Json::object([
        ("id", Json::text(&request.id)),
        ("lane", Json::text(&request.lane)),
        ("checkpoint", Json::text(&request.checkpoint)),
        ("version", Json::text(request.version.to_string())),
        ("bundle", Json::text(request.bundle.to_string())),
        ("message", Json::text(&request.message)),
        ("status", Json::text("recorded")),
        ("approval_authority", Json::Bool(false)),
    ])
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

fn file_deletion_summary(id: &str, result: &super::FileDeletionResult) -> Json {
    Json::object([
        ("schema", Json::text("mesh.fleet-file-deletion/v1")),
        ("request", Json::text(id)),
        ("operation", Json::text(result.operation.to_string())),
        ("workspace_digest", Json::text(&result.workspace_digest)),
        ("settled", Json::Bool(result.settled)),
        ("approval_authority", Json::Bool(false)),
    ])
}

#[cfg(all(test, target_os = "macos"))]
#[path = "service_deletion_tests.rs"]
mod deletion_tests;
