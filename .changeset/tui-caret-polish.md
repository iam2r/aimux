---
aimux: patch
---
Polish TUI input carets to match cc-switch-cli: the caret now underlines the character under the cursor instead of inserting an underscore glyph that shifts text, and non-text fields (yes/no cycle fields, readonly rows, kept secrets) no longer render a movable cursor at all — they highlight as accent-colored values instead.
