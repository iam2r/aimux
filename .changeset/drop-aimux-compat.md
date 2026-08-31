---
apmux: patch
---
Drop all aimux migration/compatibility code: the one-time `~/.aimux` → `~/.apmux` config-dir migration, the `aimux-sync` → `apmux-sync` WebDAV namespace migration, `AIMUX_*` env-var fallbacks, and the `aimux-*` release-asset aliases are all removed. Existing users have already migrated; pre-rename data is only mentioned in the CHANGELOG now. CI's PR policy gate no longer blocks the test jobs — fmt/clippy/tests always run on every PR regardless of the gate outcome.
