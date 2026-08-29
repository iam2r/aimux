---
aimux: patch
---

WebDAV `Push` and `Pull` now go through a confirmation popup instead of firing immediately on `p` / `u`. The popup shows the remote URL and the timestamp of the last successful sync (or "never" / "从未" on a first run); press `y` / `Enter` to proceed, `n` / `Esc` to cancel. Mirrors the existing `ConfirmDelete` / `ConfirmRestore` flow and the queue→confirm pattern used for write operations elsewhere.
