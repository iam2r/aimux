---
aimux: minor
---

Claude providers now carry a model catalog with `target_model_id`, the Anthropic model ID each row proxies (e.g. `claude-sonnet-4-6`). When the target is in the known-id table extracted from the installed Claude Code, the adapter writes the Anthropic ID into `ANTHROPIC_*_MODEL` and emits a `modelOverrides[target] = <row id>` entry so the proxy id is sent at request time — silences the "unrecognised model" warning for gateway-routed models and lets each slot pick its own real Claude window. `CLAUDE_CODE_MAX_CONTEXT_TOKENS` is set to the min of all non-empty `context_window` values. A new `unknown_model_reactive` quick item toggles `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1` for the wait-for-the-API fallback. Store-load migration seeds an empty catalog from `provider.model` and per-slot values; the official "Claude Official" row keeps its strip behavior and is unaffected.
