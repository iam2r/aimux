---
apmux: minor
---

Rename project from `aimux` to `apmux` with full auto-migration:

- Binary, config dir (`~/.aimux` → `~/.apmux`), env vars (`AIMUX_*` → `APMUX_*`), and WebDAV namespace (`aimux-sync` → `apmux-sync`) all follow the new name. A one-time migration moves existing config on first run; WebDAV data is auto-copied to the new namespace on first sync.
- Data-layer JSON fields stay package-free (no `aimux` strings inside store/webdav/manifest payloads) so future renames don't have to touch user data. The only package-coupled surface is the folder namespace, which is the legitimate place for it.
- Internal identifiers (`Paths.aimux_dir`, etc.) renamed to neutral names (`config_dir`, etc.) to avoid renaming churn in the future.
- `update` accepts legacy `aimux/vX.Y.Z` tags; release assets are published as `apmux-*` with `aimux-*` aliases for installs that haven't yet upgraded, so pre-rename binaries keep receiving updates.
- `name::pkg!()` / `name::envpref!()` are the single source of truth for the package name — everything else is derived by concatenation.
