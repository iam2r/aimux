---
aimux: patch
---

`CatalogEditor` now rejects sub-1000 values for `context_window` and `max_tokens` (treated as None) so a stray "1" placeholder can't pollute the catalog-wide `min()` that drives `CLAUDE_CODE_MAX_CONTEXT_TOKENS`. Deleting a row that owned slot bindings surfaces a status-bar message reporting how many slots were cleared, so the user isn't left wondering where sonnet went. The Claude modelOverrides path also documents its last-wins behaviour when two rows map to the same Anthropic target. Doc status banner is updated to "v1 implemented (v0.1.11+, awaiting review)" to match the current state.
