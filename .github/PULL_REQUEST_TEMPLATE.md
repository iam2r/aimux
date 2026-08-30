<!-- Title: conventional style, e.g. `fix(tui): accept bracketed paste` -->
<!-- Base branch: `develop` (main is release-only) -->

## Summary

<!-- What does this PR change, in 1–3 sentences? -->

## Motivation

<!-- Link the issue (`Fixes #123`) or describe the problem. -->

## Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] Considered Windows behavior (input/rendering/paths changes)
- [ ] Tests added/updated for parser or behavior fixes

<!-- No change file needed: maintainers add it when cutting a release. -->
