---
aimux: minor
---
New "aimux status" command reads each app's live config back (who is actually active, which model, where the key lives) and reconciles it against the store, surfacing drift from hand edits or other tools: states ok / drift / external / native / missing, with --json for scripts.
