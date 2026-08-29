---
aimux: patch
---

The catalog editor now distinguishes actual row deletes from slot-popover unassigns via explicit `pending_dropped_slots` and `deleted_default_to` fields on the editor. Previously the app's status bar fired "row deleted" any time `slot_owner.len()` shrank, which incorrectly fired on popover toggle-off. Deleting the Default row also now surfaces a "Default moved to <id>" status instead of silently reassigning the default to the new tail row.
