---
aimux: minor
---
The speedtest and trial launch land in the TUI: `t` probes the selected provider's endpoint from a background thread and reports latency plus HTTP status in the status bar ("Agate: 478 ms (HTTP 200)", official rows are rejected up front), and `o` hands the terminal over to a real trial run of the selected provider — the screen is restored when it exits with "Trial of Agate finished (exit 0) — live configs untouched". Both keys appear in the key bar and help sheet from the shared hint table; live configs are never touched by either action.
