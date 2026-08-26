---
aimux: minor
---
Key hints now come from one table. The status-bar hint, the help sheet, and the dispatcher are all generated from a single per-page key vocabulary (`HINTS` in keymap), so the shown keys can no longer drift from the real handlers — a consistency test locks every row to `map_key`. The Providers/Data/Settings help sheets (en + zh) are rendered from that table with Backups/Sync section groups preserved, replacing six hand-maintained i18n blocks.
