---
aimux: patch
---

Catalog popover multi-select: the Slots and Target-model-id popovers now use checkbox semantics like the rest of the TUI — `Space` toggles the item under the cursor without closing the popover (so several slots can be assigned in one visit), `Enter` commits and closes, `Esc` cancels unchanged. The Target picker shows a `[x]` mark for the space-marked id and commits the mark (not wherever the cursor idled), and the hint bar switches to popover-specific keys while one is open.
