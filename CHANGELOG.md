# Changelog

All notable changes to this project are documented here.

## 0.1.17 (2026-08-29)

### Fixes

- `KNOWN_CLAUDE_MODEL_IDS` gains the 5-series tier models (`claude-sonnet-5`, `claude-opus-5`, `claude-fable-5`, `claude-mythos-5`) and the legacy 3.x names (`claude-3-5-sonnet`, `claude-3-7-sonnet`) from the CLI's own known-models list, and now deliberately excludes dated snapshot IDs (`claude-sonnet-4-5-20250929` style) — undated aliases always track the current snapshot and work the same as `modelOverrides` keys, while dated IDs go stale.

## 0.1.16 (2026-08-29)

### Fixes

- Catalog popover multi-select: the Slots and Target-model-id popovers now use checkbox semantics like the rest of the TUI — `Space` toggles the item under the cursor without closing the popover (so several slots can be assigned in one visit), `Enter` commits and closes, `Esc` cancels unchanged. The Target picker shows a `[x]` mark for the space-marked id and commits the mark (not wherever the cursor idled), and the hint bar switches to popover-specific keys while one is open.

## 0.1.15 (2026-08-29)

### Fixes

- WebDAV `Push` and `Pull` now go through a confirmation popup instead of firing immediately on `p` / `u`. The popup shows the remote URL and the timestamp of the last successful sync (or "never" / "从未" on a first run); press `y` / `Enter` to proceed, `n` / `Esc` to cancel. Mirrors the existing `ConfirmDelete` / `ConfirmRestore` flow and the queue→confirm pattern used for write operations elsewhere.
- WebDAV `GET` now accepts HTTP `206 Partial Content` alongside `200 OK` when reading the remote manifest/store. Some WebDAV servers (notably the one behind `webdav.iamrazo.eu.org` and a few nginx + gzip/brotli frontends) return `206` for a plain full-resource GET and a `Content-Range` covering the whole body; the previous strict `match 200` only path therefore rejected the second `aimux sync push` (first push writes manifest.json → second push reads it and bails on `HTTP 206`). Mirrors cc-switch's `resp.status().is_success()` handling.

## 0.1.14 (2026-08-29)

### Fixes

- Catalog editor fixes for the Claude slot/target-model columns: the header now shows the translated "Target model id" label instead of the raw `field.model_overrides` key, the slot-assignment and target-model-id popovers are now actually rendered (they previously opened invisibly and swallowed every keypress), and the Slots / Target-model-id grid columns display their current values (slot aliases and the chosen target id) instead of always rendering empty cells.

## 0.1.13 (2026-08-29)

### Fixes

- The catalog editor now distinguishes actual row deletes from slot-popover unassigns via explicit `pending_dropped_slots` and `deleted_default_to` fields on the editor. Previously the app's status bar fired "row deleted" any time `slot_owner.len()` shrank, which incorrectly fired on popover toggle-off. Deleting the Default row also now surfaces a "Default moved to <id>" status instead of silently reassigning the default to the new tail row.

## 0.1.12 (2026-08-29)

### Fixes

- `CatalogEditor` now rejects sub-1000 values for `context_window` and `max_tokens` (treated as None) so a stray "1" placeholder can't pollute the catalog-wide `min()` that drives `CLAUDE_CODE_MAX_CONTEXT_TOKENS`. Deleting a row that owned slot bindings surfaces a status-bar message reporting how many slots were cleared, so the user isn't left wondering where sonnet went. The Claude modelOverrides path also documents its last-wins behaviour when two rows map to the same Anthropic target. Doc status banner is updated to "v1 implemented (v0.1.11+, awaiting review)" to match the current state.

## 0.1.11 (2026-08-29)

### Features

- Claude providers now carry a model catalog with `target_model_id`, the Anthropic model ID each row proxies (e.g. `claude-sonnet-4-6`). When the target is in the known-id table extracted from the installed Claude Code, the adapter writes the Anthropic ID into `ANTHROPIC_*_MODEL` and emits a `modelOverrides[target] = <row id>` entry so the proxy id is sent at request time — silences the "unrecognised model" warning for gateway-routed models and lets each slot pick its own real Claude window. `CLAUDE_CODE_MAX_CONTEXT_TOKENS` is set to the min of all non-empty `context_window` values. A new `unknown_model_reactive` quick item toggles `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1` for the wait-for-the-API fallback. Store-load migration seeds an empty catalog from `provider.model` and per-slot values; the official "Claude Official" row keeps its strip behavior and is unaffected.

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
