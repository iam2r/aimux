# Changelog

All notable changes to this project are documented here.

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
