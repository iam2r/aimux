---
apmux: patch
---
Self-update and the installers now take a bare version like `v0.1.20` (or `0.1.20`); the internal `apmux/` tag prefix is applied when querying GitHub. A version that no longer exists (e.g. a pre-rename `v0.1.18`, or `aimux/vX.Y.Z` passed to install.sh/install.ps1) no longer errors: both the CLI and the install scripts print a note and fall back to the latest release instead, so any well-formed version spec installs something. `update --check --json` reports the new `targetVersion` field (user-facing `vX.Y.Z`).
