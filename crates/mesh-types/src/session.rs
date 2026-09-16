//! Activity sessions.

use crate::actor::Timestamp;
use crate::entity_id::{CapabilityId, SessionId, WorkspaceId};
use crate::record_id::{ActorId, HeadId};

/// A bounded interval of one actor's activity, used to group read observations and ChangeSets for
/// attribution.
///
/// Automatically created and never user-managed — plan §4.2. The intent string is optional
/// precisely because requiring it would turn an automatic record into a prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivitySession {
    id: SessionId,
    workspace_id: WorkspaceId,
    actor_id: ActorId,
    base_head: HeadId,
    optional_intent: Option<String>,
    started_at: Timestamp,
    last_activity_at: Timestamp,
    capability_id: CapabilityId,
}

impl ActivitySession {
    /// Open a session. `last_activity_at` starts equal to `started_at`.
    #[must_use]
    pub fn open(
        id: SessionId,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        base_head: HeadId,
        started_at: Timestamp,
        capability_id: CapabilityId,
    ) -> Self {
        Self {
            id,
            workspace_id,
            actor_id,
            base_head,
            optional_intent: None,
            started_at,
            last_activity_at: started_at,
            capability_id,
        }
    }

    /// The same session with an intent recorded.
    #[must_use]
    pub fn with_intent(self, intent: String) -> Self {
        Self {
            optional_intent: Some(intent),
            ..self
        }
    }

    /// The same session, touched at `at`.
    ///
    /// Monotonic: an earlier timestamp leaves the recorded activity where it was, so an
    /// out-of-order event cannot move a session backwards.
    #[must_use]
    pub fn touched_at(self, at: Timestamp) -> Self {
        Self {
            last_activity_at: at.max(self.last_activity_at),
            ..self
        }
    }

    /// The session identifier.
    #[must_use]
    pub const fn id(&self) -> SessionId {
        self.id
    }

    /// The workspace this session belongs to.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    /// The actor whose activity this is.
    #[must_use]
    pub const fn actor_id(&self) -> ActorId {
        self.actor_id
    }

    /// The head the session started from.
    #[must_use]
    pub const fn base_head(&self) -> HeadId {
        self.base_head
    }

    /// The optional intent the actor declared.
    #[must_use]
    pub fn optional_intent(&self) -> Option<&str> {
        self.optional_intent.as_deref()
    }

    /// When the session started.
    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// When the session was last active.
    #[must_use]
    pub const fn last_activity_at(&self) -> Timestamp {
        self.last_activity_at
    }

    /// The capability the session acts under.
    #[must_use]
    pub const fn capability_id(&self) -> CapabilityId {
        self.capability_id
    }
}
