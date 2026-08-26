---
aimux: minor
---
Name-first UX: providers are addressed by display name everywhere (CLI, TUI, list output); the generated id is now an internal alias only. Fixed Codex rescue leaving requires_openai_auth rows without the shared auth.json key (switch failed with "api_key must not be empty"), and renaming a provider now moves its live slot table instead of leaving a stale entry.
