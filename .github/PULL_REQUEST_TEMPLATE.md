<!-- Title: conventional style, e.g. `fix(tui): accept bracketed paste` -->

## Summary

<!-- What does this PR change, in 1–3 sentences? -->

## Motivation

<!-- Link the issue (`Fixes #123`) or describe the problem. -->

## Change file

- [ ] Added `.changeset/<slug>.md` with the right bump (`patch` / `minor` / `major`)
      — skip only for tests/docs/CI-only changes.

## Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] Considered Windows behavior (input/rendering/paths changes)
- [ ] Tests added/updated for parser or behavior fixes
