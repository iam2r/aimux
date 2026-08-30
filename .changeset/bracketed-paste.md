---
aimux: patch
---
TUI text inputs now handle bracketed paste: a paste arrives as one event and is inserted at the cursor with control characters stripped (multi-line paste keeps newlines in the snippet JSON editor), instead of replaying as a keystroke flood — which on slow terminals made input visibly fall behind and keep replaying after the paste finished. The event loop also drains already-queued input before repainting, so paste floods and held auto-repeat keys redraw in one pass instead of one frame per character.
