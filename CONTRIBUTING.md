# Contributing to aimux

Thanks for your interest in contributing! This guide walks you through the
workflow. Releases are fully automated — you never need to touch version
numbers or the changelog.

## Quick start

```bash
git clone https://github.com/iam2r/aimux.git   # or your fork
cd aimux
cargo test          # unit tests
cargo fmt --check   # formatting (CI enforces)
cargo clippy --all-targets -- -D warnings   # lints (CI enforces)
```

CI runs on every PR: `fmt` + `clippy` + `test` on Linux, and `test` on
Windows. Please run the same locally before pushing.

## Making changes

1. Fork the repo and create a branch from `main`:
   `git checkout -b fix/my-fix` or `feat/my-feature`.
2. Keep PRs small and focused — one fix or feature per PR.
3. Commit messages follow conventional style:
   `fix(tui): accept bracketed paste`, `feat(models): ...`.
   Type + optional scope, lowercase subject. Commit messages are for humans;
   releases are driven by change files (below), not by commits.
4. Push and open a PR against `main`. A maintainer reviews and merges with a
   merge commit.

## Required: add a change file

Every user-visible change needs a **change file** in `.changeset/`, or the
release bot skips your change when cutting the next version.

Create `.changeset/<short-slug>.md`:

```markdown
---
aimux: patch
---
One-line summary shown in the changelog.
```

Pick the bump type:

| Bump    | When                                                       |
| ------- | ---------------------------------------------------------- |
| `patch` | Bug fixes, no new behavior                                  |
| `minor` | New features, new flags, UI additions                       |
| `major` | Breaking changes (rare; discuss in an issue first)          |

Internal-only changes (tests, docs, CI) don't need a change file.
See [knope.toml](knope.toml) for the exact format the bot consumes.

## What happens after merge (fully automated)

1. The release bot consumes pending change files on a `release` branch,
   bumps `Cargo.toml` / `Cargo.lock`, updates `CHANGELOG.md`.
2. It opens a Release PR and auto-merges it.
3. It tags `aimux/vX.Y.Z` and builds binaries for all platforms
   (GitHub Release assets).

You do nothing here — merging your PR is enough.

## Reporting bugs / requesting features

Please use the issue templates:

- **Bug report**: include your OS/terminal, aimux version (`aimux --version`
  or the installed tag), and steps to reproduce. TUI bugs: mention your
  terminal emulator and whether it's over SSH.
- **Feature request**: describe the problem you're trying to solve, not just
  the solution.

## Platform notes

aimux is a cross-platform TUI (macOS / Linux / Windows). When touching input
handling, rendering, or paths, consider Windows behavior — CI runs the test
suite there. Add tests for parser-level fixes (see `src/speedtest.rs` tests
for examples of deterministic probing).

## License

By contributing you agree that your contributions are licensed under the
same license as the project (see [LICENSE](LICENSE)).
