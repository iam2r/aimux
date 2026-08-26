---
aimux: patch
---
Rework the model-catalog editor layout: column widths now adapt to content (long ids no longer collide with later columns), entering edit mode keeps every cell at a fixed width with a tail window so rows never shift, and the popup widens to fit the grid. A regression test locks the alignment between idle and edited states.
