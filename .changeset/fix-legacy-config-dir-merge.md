---
apmux: patch
---

fix(legacy): merge missing files from the old `~/.aimux` config dir into `~/.apmux` when both exist

The previous migration only renamed `~/.aimux` to `~/.apmux` when the new
directory was missing. If anything had already created `~/.apmux` first
(for example the new binary's first `apmux status` call creates the
empty dir before `webdav.json`/`settings.json` ever get a chance to be
read), the old directory was left untouched and the user's WebDAV
configuration silently disappeared from the running tool.

The migration now:

- keeps the atomic `rename` path when only the old directory exists;
- when both directories exist, copies every entry from `old` into `new`
  for which `new` does not already have a same-named counterpart
  (files, nested files, and empty subtrees alike);
- never overwrites a same-named entry in `new` so the new directory
  keeps authoritative state;
- leaves the old directory in place and prints a warning so the user
  can clean it up by hand once they have confirmed everything looks
  right.

A new unit test (`migrate_merges_missing_files_when_both_dirs_exist`)
covers the merged case with overlapping, old-only, new-only, and
nested entries.
