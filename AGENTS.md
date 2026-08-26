# AGENTS.md

Guidance for Codex, Claude, Grok, and other coding agents working in this repo.

aimux switches providers for Claude Code, Codex, OpenCode, and Pi Coding Agent: one JSON store, per-app live config writes, rotating local backups, WebDAV cloud sync, a TUI, and `aimux import` for migrating from cc-switch. No GUI, no daemon.

## Commands

```bash
cargo test                              # all suites must pass
cargo fmt                               # before every commit
cargo clippy --all-targets -- -D warnings
```

Releases are changeset-driven (Knope), not commit-message-driven: add a `.changeset/<name>.md` (frontmatter `aimux: minor|patch|major`) to your PR; CI consumes it into a Release PR that bumps `Cargo.toml`/`Cargo.lock`/`CHANGELOG.md`, tags `aimux/v{version}`, and `release.yml` builds installers (Linux musl, macOS universal, Windows zip). Never bump the version by hand.

Tests must never touch host configs. Use `Paths::for_test` (or set `AIMUX_CONFIG_DIR`); never write `~/.claude`, `~/.codex`, `~/.config/opencode`, or `~/.pi`.

## Architecture

- `main.rs` — clap CLI; every command loads through `load_store()`.
- `store.rs` — **JSON SSOT** (`~/.aimux/store.json`). IndexMap keeps insertion order; official rows are pinned first. Unknown-version stores are rejected.
- `paths.rs` — all directory resolution (`AIMUX_CONFIG_DIR` override).
- `adapter/` — registry of four adapters (Claude, Codex, OpenCode, Pi). Each declares its form fields, quick toggles, model UI (catalog vs slots), snippet syntax, apply/remove logic, slot clearing, and live-config rescue.
- `switch.rs` — the only path that writes live configs (`use_provider`); TUI Enter calls the same function as the CLI.
- `settings.rs` — apps mode (auto/manual) + language. Auto detection = CLI binary on `PATH`, nothing else.
- `backup.rs`, `cloud.rs`, `webdav.rs` — snapshot rotation and WebDAV sync under the built-in namespace `aimux-sync`.
- `tui/` — ratatui app. Copy goes through `i18n.rs` (English default, zh optional); never hardcode Chinese in widgets; never `eprintln` inside the TUI.
- `update.rs` — self-update from GitHub Releases with SHA-256 verification.

## Core invariants

- **Owned-field merge**: only fields aimux owns are overwritten in live files; unknown user keys survive every apply. Snippets merge first, owned fields win.
- **Slot key = provider display name** (`Provider::slot_key()`): written as Codex `[model_providers."Name"]`, OpenCode `provider."Name"`, Pi `providers."Name"`. `Store::slot_keys` remembers what each app's live config currently holds so a switch can retire the old table.
- **Official rows** (`claude-official`, `codex-official`) are seeded idempotently, pinned to the top, and cannot be edited or deleted. Switching to one hands the CLI back to its native login (strip aimux-owned env keys / remove the injected provider block and `OPENAI_API_KEY`; OAuth tokens are never touched).
- **First run rescue**: when `store.json` doesn't exist, `rescue_from_live` adopts hand-configured providers from each agent's live files (base URL/key/model/catalog) instead of making users re-enter them. Nothing is written if nothing is found.
- **Codex catalog rows** are cloned from the embedded native template (`codex_native_responses.json`) with stored fields overlaid. Never strip `base_instructions` — Codex refuses catalog files without it. Empty catalogs omit the file and the `model_catalog_json` key.
- **Snippets**: stored as JSON always; the editor surface syntax is per adapter (JSON everywhere except Codex, which edits TOML sections via toml_edit).
- **Secrets**: masked in output unless `AIMUX_SHOW_SECRETS=1`. New live files get mode 0600; existing modes are kept.
- **Rename**: everything machine-read lives in `src/name.rs`; a rename touches it plus `Cargo.toml`, install scripts, and prose docs.

## Scope

In: provider switching for the four CLIs, local backups, WebDAV sync, cc-switch import, self-update, TUI.

Out: proxies, MCP/skills/prompt management, sessions, OAuth managers, daemons, other agent CLIs.
