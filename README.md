# aimux

Lightweight local tool to **switch providers** for Claude Code, Codex, OpenCode, and Pi Coding Agent, with rotating local backups and WebDAV cloud backup. No GUI. Run `aimux` with no arguments for the TUI, or use subcommands in scripts.

Out of scope: proxy, MCP management, skills, sessions/usage, OAuth account managers, daemon, Gemini CLI.

[简体中文](README_ZH.md)

## Install

Linux / macOS:

```bash
curl -fsSL https://github.com/iam2r/aimux/releases/latest/download/install.sh | bash
```

Windows (PowerShell):

```powershell
irm https://github.com/iam2r/aimux/releases/latest/download/install.ps1 | iex
```

The Unix installer puts `aimux` in `~/.local/bin` (override with `AIMUX_INSTALL_DIR`). If that directory is not on `PATH`, it appends a managed block to `~/.bashrc` / `~/.zshrc` / fish config. Windows installs to `%LOCALAPPDATA%\aimux\bin` and adds it to the user `Path`. Linux downloads are static musl builds. After install, `aimux update` replaces this binary from GitHub Releases (`aimux update --check` only reports).

```bash
# specific version
curl -fsSL https://github.com/iam2r/aimux/releases/latest/download/install.sh | bash -s -- v0.1.0
AIMUX_SKIP_PATH=1 bash install.sh   # install only; do not edit shell rc
```

### Releases

Releases are maintainer-driven, still powered by **changeset files** (via
[Knope](https://knope.tech)), not commit messages:

1. All development happens on the `develop` branch (`main` is release-only).
   Contributors never add change files — maintainers write them when deciding
   to cut a release: `.changeset/<name>.md` on `develop`:

   ```markdown
   ---
   aimux: minor        # minor | patch | major
   ---
   One-line summary for the changelog.
   ```

2. Pushing change files to `develop` makes CI consume them, open a
   **Release PR** (develop → main) that bumps `Cargo.toml` and `CHANGELOG.md`,
   and auto-merge it.
3. The same run tags `aimux/vX.Y.Z`, builds installers and archives, and
   back-merges `main` into `develop`. No local tooling required.

From source (Rust stable):

```bash
git clone https://github.com/iam2r/aimux.git
cd aimux
cargo install --path .
# or: cargo build --release   → target/release/aimux
```

Config lives in `$HOME/.aimux` (`store.json`, `webdav.json`, `backups/`). Override with `AIMUX_CONFIG_DIR`.

## Migrate from cc-switch

`aimux import` copies Claude / Codex / OpenCode providers from `~/.cc-switch/cc-switch.db` into `store.json`, and WebDAV credentials from `~/.cc-switch/settings.json` into `webdav.json` (`baseUrl` only; cc-switch's `remoteRoot` / `cc-switch-sync` is ignored). Skips Gemini, Grok, and official empties. Does **not** write live CLI files, MKCOL, or pull the remote snapshot. Pi and other apps already in the store are kept. Existing `webdav.json` is left alone unless you pass `--force`.

```bash
aimux import --dry-run
aimux import
aimux list
aimux use <name>
```

Default is merge (skip existing ids). `--force` overwrites colliding ids, `current` for imported apps, and `webdav.json` (timestamp backup first). Keys are masked in the report.

## Usage

```bash
aimux                              # TUI
aimux list [--app <app>] [--json]
aimux current [--app <app>] [--json]
aimux use <name> [--app <app>]
aimux add --app <app> --name <name> --base-url <url> --api-key <key> \
        [--model <id>] [--extra key=value]... [--apply-snippet]
aimux edit <name> [--app] [--name] [--base-url] [--api-key] \
        [--model <id> | --clear-model] [--extra key=value]... \
        [--apply-snippet | --no-apply-snippet]
aimux snippet <name> [--app <app>] [--set '<json>' | --clear]
aimux delete <name> [--app] [--yes]
aimux backup [--name <name>]
aimux restore <name> [--yes] [--no-apply]
aimux backups
aimux sync setup --url <webdav-root> --username <user> --password <pass>
aimux sync push [--force]
aimux sync pull [--force]
aimux sync status
aimux import [--db <path>] [--settings <path>] [--dry-run] [--force]
aimux update [--version <tag>]
aimux update --check [--json]
```

`<app>`: `claude` / `codex` / `opencode` / `pi`. Exit codes: `0` success, `1` user/validation error, `2` I/O or network.

`list` / `current` mask API keys by default (first 4 + `…` + last 4; shorter than 8 → all `*`). `--json` is also masked. `AIMUX_SHOW_SECRETS=1` prints full keys — **dangerous**; they land in scrollback and CI logs. Local debugging only.

The CLI is non-interactive (no prompts). Missing required flags exit non-zero. Use the TUI for interactive editing.

## Language

English is the default for the TUI, docs, and clap help.

| Source | Effect |
|--------|--------|
| `--lang en` / `--lang zh` | Explicit (wins) |
| `AIMUX_LANG` | Same values (`en`, `zh`, `zh_CN`, …) |
| TUI Settings (`g`) | Saved in `settings.json` |
| `LANG` / `LC_ALL` | `zh*` → Chinese TUI; anything else → English |

CLI clap help stays English so scripts and issue reports stay greppable. TUI strings go through `src/i18n.rs`.

## TUI keys

Bare `aimux` opens the TUI. `?` shows help for the current page.

Provider list:

| Key | Action |
|-----|--------|
| `[` `]` or Tab | Previous / next app |
| `j` `k` or ↑ ↓ | Move in the list |
| `Enter` | Switch provider |
| `a` | Add |
| `e` | Edit |
| `d` | Delete (confirm) |
| `b` | Backup now (timestamp) |
| `r` | Backups page |
| `s` | Sync page |
| `g` | Settings (language, auto/manual app detection) |
| `?` | Help |
| `q` / `Esc` | Quit / close overlay |

Forms: `Tab` / ↑ ↓ change field, Space cycles options **or fetches the model list** on the Model field, Enter/Space on **Catalog**, **Slots**, or **Snippet** opens that editor, Enter on other fields submits, Esc cancels. Secrets are masked; leave empty on edit to keep the current value. Leave an optional model empty to clear it; a required model can’t be empty.

Catalog apps (OpenCode, Pi, Codex): Space on Model fetches, Space toggles rows, Enter opens a catalog editor (`id` / `label` / `context_window` / `max_tokens`; Codex has no max-tokens column). Claude: the Slots field opens the table; Space picks one id for the focused slot; `a` copies that id onto the other slots including default.

**Snippet** on the add/edit form is per-provider JSON. **Built-in checkboxes** (Claude hide-attribution / Teammates / Tool Search / effort / auto-upgrade; Codex Goal mode) sit above the JSON body and compose it. Opt in with **Apply snippet**, or `aimux add/edit --apply-snippet`. The snippet merges first; owned fields win. `aimux snippet <name>` prints/sets/clears that provider’s JSON.

**Official rows** (`claude-official`, `codex-official`) are built in and seeded automatically: switching to one hands the CLI back to its native subscription (Claude Code login / ChatGPT login). They cannot be edited or deleted — pick one and press Enter to switch.

Backups: Enter restores (confirm), `b` snapshots now, Esc returns.

Sync: `e` setup, `p` push, `u` pull. While a job runs, a static **Syncing…** overlay is shown (no spinner); keys other than `q` are ignored.

Settings (`g`): Space/Enter changes the focused row. App detection defaults to **auto**: only a CLI found on `PATH` counts — leftover config folders do not count. Switch to **manual** to show/hide each app. Language is saved in `$AIMUX_CONFIG_DIR/settings.json`.

## WebDAV

`--url` is the WebDAV root. aimux always stores files under the built-in namespace `aimux-sync`. The TUI shows that namespace on its own row; it cannot be edited.

```bash
aimux sync setup \
  --url 'https://webdav.example.com/' \
  --username 'you' \
  --password '<password>'
```

`--url` / `--username` / `--password` are always required. Non-localhost `http://` is rejected. Setup MKCOLs `{url}/aimux-sync` and stores **the root URL you submitted**.

TUI: `s` → `e`, fill URL / user / password. Namespace is a separate read-only row (`aimux-sync`).

`push` / `pull` take a timestamp backup before overwriting the store. On conflict, push is refused; `pull` or `--force`. `status` never prints the password.

Credentials: `$AIMUX_CONFIG_DIR/webdav.json`, mode `0600`.

## Project `opencode.json` shadowing

aimux writes **global** live files only:

| App | Global live |
|-----|-------------|
| Claude | `~/.claude/settings.json` (directory must already exist) |
| Codex | `~/.codex/config.toml` + `auth.json` |
| OpenCode | `~/.config/opencode/opencode.json` |
| Pi | `~/.pi/agent/models.json` + `settings.json` |

OpenCode itself prefers a **project** `opencode.json` / `opencode.jsonc`, which can shadow the global file. aimux does not scan or rewrite project files. Pi: project `.pi/` is never touched.

If the target CLI is not initialized (resolved config dir missing), aimux records `current` but **does not** create the directory or write live files.

## Do not run concurrent writers

v1 has **no** `store.json` lock. Two parallel `aimux use` / `edit` / `delete` / `restore` / `sync push|pull` processes can lose updates (atomic rename only prevents torn writes).

## Secrets and permissions

- `store.json`, `webdav.json`, backups: Unix `chmod 0600` after every save; aimux dir `0700`.
- Secret live files (Claude `settings.json`, Codex `auth.json`, OpenCode `opencode.json`, Pi `models.json`): new files `0600`; existing mode kept.
- TUI/CLI mask API keys by default. Logs never record `api_key`, passwords, or Authorization headers.

## Test isolation

Tests inject tempfile `Paths` and must not write the host:

- `~/.claude`
- `~/.codex`
- `~/.config/opencode`
- `~/.pi`
- real `~/.aimux`

`cargo test` panics if an apply would hit those paths and `Paths.home` is not a tempfile. Do not use process `HOME` as the main isolation mechanism.

```bash
cargo test
cargo fmt
cargo clippy --all-targets -- -D warnings
```

`0600` assertions are `#[cfg(unix)]`; Windows skips mode checks.

## Windows

Prefer the PowerShell one-liner above. GitHub Actions: PRs get a policy gate plus `fmt` + `clippy` + `test` on `ubuntu-latest` and `test` on Windows; the release bot tags `aimux/vX.Y.Z` and publishes Linux/macOS/Windows release assets (a manual `v*` tag push triggers the same build).

On Windows:

- Atomic write is still same-dir tmp + rename (replaces an existing target).
- `chmod 0600` / `0700` **failures are ignored**; v1 does not set Windows ACLs.
- Tests inject `Paths`; they do not rely on process `HOME` / `USERPROFILE`.

## Contributing

Bug reports, feature requests, and PRs are welcome — see
[CONTRIBUTING.md](CONTRIBUTING.md) for the workflow. All PRs target the
`develop` branch; CI runs the checks automatically, and you don't need to
add change files.

## License

MIT.
