---
aimux: minor
---
Post-switch restart hints and a new "aimux test <provider>" speedtest. Every CLI reads its config at startup, so switches now say so: the TUI status bar appends "restart to apply" and the CLI prints which app needs restarting (works even without --app). The speedtest probes a provider base_url with a warm-up + timed request (cc-switch-cli approach) and reports latency plus HTTP status; official rows explain there is nothing to probe.
