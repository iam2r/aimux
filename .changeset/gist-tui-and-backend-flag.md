---
apmux: minor
---
Sync gains backend flags and Gist lands in the TUI. `apmux sync setup` takes `--backend webdav` (default, unchanged `--url/--username/--password`) or `--backend gist` (`--token`, optional `--gist <id-or-url>`); `sync push|pull|status` accept `--backend` too, replacing the `sync gist …` subcommand. On the TUI Data page the Sync panel now has WebDAV/Gist tabs — `Tab` switches, `e` sets up the active backend, `p` pushes, `u` pulls, both backends work side by side.
