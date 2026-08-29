---
aimux: patch
---

`KNOWN_CLAUDE_MODEL_IDS` gains the 5-series tier models (`claude-sonnet-5`, `claude-opus-5`, `claude-fable-5`, `claude-mythos-5`) and the legacy 3.x names (`claude-3-5-sonnet`, `claude-3-7-sonnet`) from the CLI's own known-models list, and now deliberately excludes dated snapshot IDs (`claude-sonnet-4-5-20250929` style) — undated aliases always track the current snapshot and work the same as `modelOverrides` keys, while dated IDs go stale.
