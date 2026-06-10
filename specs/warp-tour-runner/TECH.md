# Warp Tour Runner — TECH.md

See `specs/warp-tour-runner/PRODUCT.md` for behavior. Behavior invariant numbers below refer to that document.

## Context
The guided tour today is agent-orchestrated: the `warp-tour` skill ([`resources/bundled/skills/warp-tour/SKILL.md @ 7911402d`](https://github.com/warpdotdev/warp/blob/7911402db11d0ff588720003cb9ca42b98bc1250/resources/bundled/skills/warp-tour/SKILL.md)) tells the agent to capture deterministic copy from `warpctrl tour <stop>` and drive surfaces with granular `warpctrl` commands. Relevant existing code, pinned to `7911402d`:

- `crates/warp_cli/src/local_control/tour/copy.rs` — pure copy-printing module; no IPC. (Originally a single `tour.rs` with a copy-only `TourCommand` enum in `mod.rs`; that work was branch-local and uncommitted at `7911402d`, so it has no pinned link, and this feature absorbed it into the `tour/` module.)
- [`crates/warp_cli/src/local_control/commands.rs (703-732) @ 7911402d`](https://github.com/warpdotdev/warp/blob/7911402db11d0ff588720003cb9ca42b98bc1250/crates/warp_cli/src/local_control/commands.rs#L703-L732) — `run_action_with_params`: discovery → instance selection → `RequestEnvelope` → `client::send_request` → prints. Every command group funnels through this one-shot helper; there is currently no way to invoke an action and get the result back as a value.
- [`crates/local_control/src/client.rs (42-89) @ 7911402d`](https://github.com/warpdotdev/warp/blob/7911402db11d0ff588720003cb9ca42b98bc1250/crates/local_control/src/client.rs#L42-L89) — blocking `send_request`; requests an exact-action scoped credential per call via the owner-authenticated broker.
- [`crates/local_control/src/catalog.rs (167-296) @ 7911402d`](https://github.com/warpdotdev/warp/blob/7911402db11d0ff588720003cb9ca42b98bc1250/crates/local_control/src/catalog.rs#L167-L296) — the 84-action catalog. Everything the tour needs already exists: `app.active`, `surface.list`, `pane.split`/`pane.list`/`pane.focus`/`pane.close`, `surface.*.open`, `theme.get`/`theme.set`/`theme.system.set`/`theme.light.set`/`theme.dark.set`, `keybinding.get`, `tab.create` (with `tab_type`), `tab.close`, `input.insert`.

Key implication: the entire feature is CLI-side orchestration in `warp_cli`. No app-bridge, protocol, or catalog changes; the security model (one exact-action credential per dispatched action) is untouched (invariant 24).

## Proposed changes
All in `crates/warp_cli/src/local_control/`, plus the bundled skill.

### 1. Action invocation refactor (`commands.rs`)
Split `run_action_with_params` into:
- `invoke_action<T: Serialize>(args: &TargetArgs, action: ActionKind, params: T) -> Result<serde_json::Value, ControlError>` — performs discovery/selection/credential/request and returns the `data` payload.
- The existing printing wrapper, which delegates to `invoke_action` and renders per `OutputFormat` (unchanged behavior for all current commands).

The tour runner and composites call `invoke_action` repeatedly within one process. Instance selection should be resolved once per tour invocation and reused; add an `invoke_action_on(instance: &InstanceRecord, ...)` variant so the tour does not re-run discovery per step.

### 2. Tour module restructure (`tour.rs` → `tour/`)
- `tour/copy.rs` — existing copy functions converted to string-returning forms (via `color_print::cformat!`) so the runner and `tour stop` can both print and embed copy in JSON; legacy copy subcommands keep printing them (invariant 21). A few lines that assumed an agent narrator are reworded to fit both narrators.
- `tour/invoker.rs` — the `ActionInvoker` trait, the live `ClientInvoker`, and pane/tab target-selector helpers. A scripted `tour/test_support.rs` double records dispatched actions for tests.
- `tour/state.rs` — `TourState`: selected instance, anchor IDs (window/tab/pane/session from `app.active`), tour pane ID, optional agent tab ID, saved `ThemeState`, visited/dropped stops. Plus `StopName` enum (`ValueEnum`) mapping to copy + the ordered list of `ActionKind` surface opens per stop (mirroring the skill's stop definitions).
- `tour/composite.rs` — `tour init` / `tour stop` / `tour finish` (invariants 17–20). Each is a sequence of `invoke_action_on` calls with results accumulated into serde structs (`TourInitResult`, `TourStopResult { copy, steps: Vec<TourStepResult>, keybindings }`, `TourFinishResult`). Tour-pane discovery after `pane.split` uses a before/after `pane.list` diff, implemented once here. Partial-failure semantics per invariant 19: record each step's `Result` and keep going; exit code derives from whether copy emission + anchor refocus succeeded.
- `tour/runner.rs` — `tour run` (invariants 1–13). A loop over a small state machine (`Menu`, `Stop(StopName)`, `AgentHandoff`, `Finish`) built on the same composite primitives. Prompts are numbered menus via `println!` + `stdin().read_line` (no new dependency, no raw mode — invariant 5). TTY detection via `std::io::IsTerminal` (invariant 3). Ctrl+C: install a `ctrlc`-style handler is overkill; instead treat `read_line` EOF/interrupt errors as the exit path and run the same best-effort cleanup (theme restore + open-state summary) in a single `finish_best_effort(&mut TourState)` used by all exit paths (invariant 12). Repo detection for invariant 11: walk up from cwd looking for `.git` (no `git` subprocess).
- Agent handoff (invariants 14–16): `tab.create` with `tab_type: agent` (params already exist as `TabCreateParams`), capture the new tab from the acknowledgement/`tab.list` diff, then `input.insert` targeting that tab with a templated prompt embedding the user's question and tour context. Never call anything that submits. Store the agent tab ID in `TourState` for cleanup gating.

### 3. CLI surface (`mod.rs`)
Extend `TourCommand` with `Run(TourRunArgs)`, `Init(TourInitArgs)`, `Stop(TourStopArgs)`, `Finish(TourFinishArgs)` alongside the existing copy variants. `TourStopArgs` takes `stop: StopName`, `--tour-pane`, `--anchor-pane`; `TourFinishArgs` takes `--tour-pane`, repeatable `--tour-tab`, `--restore-theme <json>`. Dispatch in `run_inner` passes `output_format` through to the composite commands (the copy variants continue to ignore it).

### 4. Skill update (`resources/bundled/skills/warp-tour/SKILL.md`)
Rewrite the workflow sections to: (a) offer `warpctrl tour run` first by staging it via Ask User Question/input rather than running it (invariant 22); (b) replace the per-stop multi-command choreography with `tour init` → `tour stop <name>` → `tour finish`, consuming the JSON results; (c) keep a short fallback section using the granular commands when `tour init --help` fails (older CLI). The skill shrinks substantially; the safety rules (never submit input, never close pre-existing targets) stay.

### 5. Spec maintenance
Update `specs/warp-control-cli/README.md` and `TECH.md` only if reviewers want the tour composites mentioned; the action catalog count (84) does not change.

Tradeoff noted: composites could instead be new server-side catalog actions (single credential, single round-trip). Rejected — it grows the allowlisted attack surface and duplicates orchestration in the app bridge for no user-visible gain; per-action credential requests are local and cheap.

## Testing and validation
Per repo convention, tests live in `<file>_tests.rs` included via `#[cfg(test)] #[path = ...]`, run with `cargo nextest run --no-fail-fast --workspace`.

- `tour/state_tests.rs` — stop→surface-action mapping completeness (every `StopName` has copy and at least one surface action); ordering with/without repo detection (invariant 11).
- `tour/composite_tests.rs` — drive composites against a mock invoker (introduce `trait ActionInvoker` implemented by the real client and a scripted test double): `tour init` partial failure shape (invariant 17), `tour stop` per-step results and exit-code rules (invariants 18–19), `tour finish` only touches passed IDs (invariant 20).
- `tour/runner_tests.rs` — state machine transitions with scripted stdin (menu → stop → skip/hint/next, EOF triggers best-effort cleanup, theme restore invoked exactly once) covering invariants 8, 9, 12; agent handoff stages `input.insert` and never any submit-like action (invariants 14–15, by asserting the full action trace of the mock invoker).
- CLI parse tests in `local_control_tests.rs` following existing patterns: new subcommands, flag conflicts, `--restore-theme` JSON parsing.
- Manual E2E (matching app + CLI bits): `cargo run -p warp --bin warp --features warp_control_cli -- --warpctrl tour run` inside a dev Warp build; walk the core tour; verify tour pane reuse, theme restore, Ctrl+C cleanup, agent handoff staging (prompt visible, not submitted), and `tour finish` close confirmations. Verify invariant 21 by running each legacy copy subcommand.
- Skill validation: run the updated `warp-tour` skill end-to-end in an agent conversation and count CLI invocations per stop (expect 1, vs 4–6 today).
- Presubmit: `./script/format` and the clippy invocation from `./script/presubmit` before the PR.

## Parallelization
Limited benefit — the runner, composites, and CLI surface all touch the same few files in `warp_cli` and share the `ActionInvoker`/`TourState` foundation, so splitting them invites conflicts. Proposed split after the foundation lands:

- Sequential first: one agent implements §1–§3 (refactor, tour module, CLI surface) with tests, on branch `zach/warp-tour-runner` in the main worktree (this is already the feature area of the current branch line).
- Then parallel: a second local agent in worktree `/Users/zach/.warp-dev/worktrees/warp_3/tour-skill` on branch `zach/warp-tour-skill`, owning only `resources/bundled/skills/warp-tour/SKILL.md` (§4) against the now-stable CLI interface; merged into the same PR via cherry-pick before review.

If wall-clock time is not a concern, doing both sequentially on one branch is simpler and is the default recommendation.
