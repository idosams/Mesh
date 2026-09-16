# Threat model

Mesh protects exact workspace state and the authority to advance a protected shared version.

## Assets

- Workspace file contents and durable private history.
- Actor isolation and per-workspace path confinement.
- Human approval credentials and exact approval receipts.
- Publication targets and Git ref authority.
- Diagnostic output and support bundles.

## Trust boundaries

- The browser layer is untrusted for filesystem and signing authority.
- The local daemon owns durable state and validates every workspace binding.
- Agent keys may create private work but cannot advance the protected shared version.
- Human approval requires a native confirmation over exact reviewed bytes.
- Native folders are ordinary filesystem surfaces and may change concurrently.

## Required protections

- Canonical digests bind reviews, approvals, exports, and recovery to exact bytes.
- Path operations remain beneath verified roots and reject links or replaced identities.
- Lost replies use read-back and idempotent recovery rather than replaying mutation.
- Approval and export authority expires when workspace identity, generation, content, or target
  changes.
- Support material is previewed and redacted before leaving the machine.
- Ambiguous filesystem observations fail closed and remain visible to the person.

## Out of scope

Mesh does not defend the machine from its own logged-in user, promise availability against
unbounded local resource exhaustion, or treat an agent as a human approver.

Report vulnerabilities privately according to [the security policy](../.github/SECURITY.md).
