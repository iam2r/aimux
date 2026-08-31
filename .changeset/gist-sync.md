---
apmux: minor
---
Cloud sync gains a GitHub Gist backend: `apmux sync gist setup <token>` creates — or finds, by the sync-format marker in the gist description — a secret gist holding the same `store.json` + `manifest.json` pair the WebDAV backend uses, seeded with the current local store. `sync gist push|pull|status` share WebDAV's conflict detection, manifest verification, local backups and re-apply logic, and `setup --gist <id-or-url>` pins an existing gist.
