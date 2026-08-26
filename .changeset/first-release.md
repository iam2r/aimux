---
aimux: minor
---
Name-first UX: providers are addressed by display name everywhere (CLI, TUI, list output); the generated id is now an internal alias only. Fixed Codex rescue leaving requires_openai_auth rows without the shared auth.json key (switch failed with "api_key must not be empty"), and renaming a provider now moves its live slot table instead of leaving a stale entry.

Self-update now resolves releases through the rate-limit-free releases page first (the GitHub REST API was hitting 403 for anonymous users), degrades gracefully to direct download URLs when the API is unavailable, accepts v0.1.1 / 0.1.1 / aimux/v0.1.1 for --version, and parses slash-prefixed tags correctly.

Snippet editor help text now advertises the actually reliable Ctrl+S shortcut instead of Ctrl+Enter.
