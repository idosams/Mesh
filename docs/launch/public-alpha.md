# Mesh public alpha

The first public alpha targets Apple-silicon Macs and is meant for testing on disposable or
backed-up projects. It is not a stable API, an automatic updater, or a hosted team service.

This technical alpha is published from one exact public commit after its test suite is green, the
packaged-window proof succeeds, the archive and checksum bind the same source revision, private
vulnerability reporting is live, there is no unresolved P0 data-safety finding, and a human makes
the explicit ship decision.

The downloadable archive is ad-hoc signed and **not notarized**. It does not provide the stable
Apple application identity required by Mesh approval, so approval and **Update original folder**
are unavailable in this build. Test exports only to a separate empty destination folder.

## Install

1. Download the delivery ZIP from the GitHub prerelease and expand it.
2. Verify the inner app archive against the included SHA-256 checksum.
3. Expand the app archive and move `Mesh.app` to `/Applications`.
4. In Finder, explicitly choose **Open** for this unnotarized technical-alpha build. Never disable
   Gatekeeper globally and never remove quarantine metadata.
5. Use only a disposable or backed-up project during the alpha.

## Uninstall

Quit Mesh, remove `/Applications/Mesh.app`, and retain application-support data until every managed
workspace you need has been exported. There is no automatic updater in this release.

Report functional problems through GitHub Issues. Report security problems privately according to
the [security policy](../../.github/SECURITY.md).
