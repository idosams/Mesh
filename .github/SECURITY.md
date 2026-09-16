# Security policy

## Reporting a vulnerability

Do not open a public issue, pull request, or discussion for a vulnerability. Use GitHub's private
vulnerability-reporting form on this repository: **Security** → **Report a vulnerability**.

Include the exact release or commit, the platform, the impact, and a reproduction that maintainers
can run. Please state any disclosure deadline and whether the issue has been shared elsewhere.

## Scope

Security-sensitive areas include:

- approval, publication, policy, and cryptographic key handling;
- actor isolation and workspace path confinement;
- the daemon, storage engine, and filesystem adapters;
- import, export, rollback, and agent-handoff custody;
- desktop IPC, local launch authority, and support-bundle redaction.

Mesh does not claim to defend a machine from its own logged-in user. Automated scanner output with
no demonstrated impact is not sufficient by itself.

## Response targets

- Human acknowledgement within three working days.
- Initial reproduction and severity assessment within ten working days.
- A fix or written remediation plan within ninety days for accepted findings.

Only the latest public alpha is supported during the prerelease period.
