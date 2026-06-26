---
name: diagnose-ci-failures-local
specializes: diagnose-ci-failures
specializes_source: warpdotdev/common-skills:.agents/skills/diagnose-ci-failures
description: Repo-specific CI-diagnosis guidance for the warp client. Layers Warp's CI check names and cargo-specific error categories on top of the generic diagnose-ci-failures skill.
---

# Repo-specific CI-diagnosis guidance for `warp`

## Prerequisite: install the parent skill

This skill specializes the core `diagnose-ci-failures` skill (named in the `specializes` frontmatter field) and is not functional on its own. Before applying its guidance, confirm the parent skill is installed and resolvable at `.agents/skills/diagnose-ci-failures/SKILL.md`. If it is missing, install it first by copying the skill directory from the source declared in the `specializes_source` frontmatter field (`warpdotdev/common-skills:.agents/skills/diagnose-ci-failures`). Then continue with the guidance below.

This file is a companion to the core `diagnose-ci-failures` skill. It does not redefine the generic workflow (verify the PR, check status with `gh`, extract logs, categorize, then produce a fix plan). It only layers the Warp client's specific CI check names and the cargo-centric error categories to look for.

## Warp client CI check names

When parsing `statusCheckRollup`, map failures to these checks:
- `Formatting + Clippy (MacOS)`
- `Formatting + Clippy (Linux)`
- `Run MacOS tests`
- `Run Linux tests`
- `Run Windows tests`
- `WASM build`
- `Check CI results` — the summary/rollup check; a failure here usually reflects one of the checks above, so trace it back to the underlying job before diagnosing.

## Cargo-specific error categories

When categorizing extracted logs, group errors into:
- **Formatting issues** — `cargo fmt -- --check` failures. Fix with `./script/format`.
- **Linting issues** — `cargo clippy` warnings/errors (note the specific lint name, e.g. `uninlined_format_args`, `dead_code`).
- **Compilation errors** — type mismatches, missing/unused imports, signature changes, non-exhaustive matches.
- **Test failures** — failing `cargo nextest`/doc tests with their names and failure reasons.
- **Platform-specific issues** — split by job: macOS / Linux / Windows test failures, and `WASM build` failures (typically `local_fs`-gating problems on the `wasm32-unknown-unknown` target).

## Notes

- Cross-reference the `fix-errors` skill (and `fix-errors-local`) for detailed resolution strategies and the exact reproduction commands for each category.
- A failure in `WASM build` almost always means filesystem-using code needs gating behind `local_fs`; reproduce locally with `cargo clippy --target wasm32-unknown-unknown --profile release-wasm-debug_assertions --no-deps`.
- If tests passed in CI but fail locally, they may be environment-specific or flaky; prefer the CI result as the source of truth.
- The validation steps in the generated fix plan should reference `./script/presubmit` as the final local gate.
