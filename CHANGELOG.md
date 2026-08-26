# Changelog

All notable changes to this project are documented here.

## 0.1.10 (2026-08-26)

### Features

- The speedtest and trial launch land in the TUI: `t` probes the selected provider's endpoint from a background thread and reports latency plus HTTP status in the status bar ("Agate: 478 ms (HTTP 200)", official rows are rejected up front), and `o` hands the terminal over to a real trial run of the selected provider — the screen is restored when it exits with "Trial of Agate finished (exit 0) — live configs untouched". Both keys appear in the key bar and help sheet from the shared hint table; live configs are never touched by either action.

## 0.1.9 (2026-08-26)

### Features

- New `aimux try <PROVIDER> [-- <cli args…>]`: trial-launch a provider without touching live configs. Each app gets a throwaway config directory selected through its official override env (CLAUDE_CONFIG_DIR / CODEX_HOME / OPENCODE_CONFIG / PI_CODING_AGENT_DIR), the real CLI runs attached to your terminal, and the temp dir is removed when it exits. Official rows report there is nothing to try; exit codes pass through.

## 0.1.8 (2026-08-26)

### Features

- Key hints now come from one table. The status-bar hint, the help sheet, and the dispatcher are all generated from a single per-page key vocabulary (`HINTS` in keymap), so the shown keys can no longer drift from the real handlers — a consistency test locks every row to `map_key`. The Providers/Data/Settings help sheets (en + zh) are rendered from that table with Backups/Sync section groups preserved, replacing six hand-maintained i18n blocks.

## 0.1.7 (2026-08-26)

### Features

- Post-switch restart hints and a new "aimux test <provider>" speedtest. Every CLI reads its config at startup, so switches now say so: the TUI status bar appends "restart to apply" and the CLI prints which app needs restarting (works even without --app). The speedtest probes a provider base_url with a warm-up + timed request (cc-switch-cli approach) and reports latency plus HTTP status; official rows explain there is nothing to probe.

## 0.1.6 (2026-08-26)

### Fixes

- Rework the model-catalog editor layout: column widths now adapt to content (long ids no longer collide with later columns), entering edit mode keeps every cell at a fixed width with a tail window so rows never shift, and the popup widens to fit the grid. A regression test locks the alignment between idle and edited states.

## 0.1.5 (2026-08-26)

### Features

- New "aimux status" command reads each app's live config back (who is actually active, which model, where the key lives) and reconciles it against the store, surfacing drift from hand edits or other tools: states ok / drift / external / native / missing, with --json for scripts.

## 0.1.4 (2026-08-26)

### Features

- The snapshot key (b) now lives exclusively inside the Data page instead of being a global shortcut; the provider list hint and help no longer advertise it.

## 0.1.3 (2026-08-26)

### Features

- Merge the scattered backup and sync surfaces into a single Data page: backups list on top (j/k select, Enter restore, b snapshot), sync status below (e setup, p push, u pull). Both r and s open the page; the list footer hint and contextual help reflect the merged layout.

## 0.1.2 (2026-08-26)

### Fixes

- Polish TUI input carets to match cc-switch-cli: the caret now underlines the character under the cursor instead of inserting an underscore glyph that shifts text, and non-text fields (yes/no cycle fields, readonly rows, kept secrets) no longer render a movable cursor at all — they highlight as accent-colored values instead.

## 0.1.1 (2026-08-26)

### Features

#### Name-first UX: providers are addressed by display name everywhere (CLI, TUI, list output); the generated id is now an internal alias only. Fixed Codex rescue leaving requires_openai_auth rows without the shared auth.json key (switch failed with "api_key must not be empty"), and renaming a provider now moves its live slot table instead of leaving a stale entry.

Self-update now resolves releases through the rate-limit-free releases page first (the GitHub REST API was hitting 403 for anonymous users), degrades gracefully to direct download URLs when the API is unavailable, accepts v0.1.1 / 0.1.1 / aimux/v0.1.1 for --version, and parses slash-prefixed tags correctly.

Snippet editor help text now advertises the actually reliable Ctrl+S shortcut instead of Ctrl+Enter.
