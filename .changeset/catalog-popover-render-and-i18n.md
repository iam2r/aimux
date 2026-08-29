---
aimux: patch
---

Catalog editor fixes for the Claude slot/target-model columns: the header now shows the translated "Target model id" label instead of the raw `field.model_overrides` key, the slot-assignment and target-model-id popovers are now actually rendered (they previously opened invisibly and swallowed every keypress), and the Slots / Target-model-id grid columns display their current values (slot aliases and the chosen target id) instead of always rendering empty cells.
