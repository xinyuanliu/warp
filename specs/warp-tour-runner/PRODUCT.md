# Warp Tour Runner — PRODUCT.md

## Summary
Make the guided Warp tour fast and free by default. A new interactive `warpctrl tour run` command drives the entire tour deterministically from a terminal pane — zero model calls, zero per-step latency — while keeping the agent available as an opt-in "escape hatch" for free-form questions. New composite `tour init` / `tour stop` / `tour cleanup` commands collapse the agent-driven tour's per-stop round-trips for users who still run the tour through the agent.

## Problem
Today the tour is driven end-to-end by an agent following the `warp-tour` skill. The copy is already deterministic (printed by `warpctrl tour <stop>`), but every menu, stop transition, surface open, and recovery step costs an agent inference round-trip. The result is noticeable latency between stops and a tour that consumes the user's paid agent requests. A pure static tour would fix both but lose flexibility; this feature keeps intelligence available on demand while making the default path deterministic.

## Goals / Non-goals
Goals:
- A complete, self-driving tour with no agent involvement on the happy path.
- The user can pull in an agent at any stop without the tour silently spending their requests.
- The agent-driven tour (via the `warp-tour` skill) becomes materially cheaper and faster through composite commands.

Non-goals:
- Non-billable agent invocations. This requires stack-wide changes and is explicitly deferred.
- A native (in-app, non-terminal) tour UI. The runner is a CLI experience; a native overlay may follow later.
- New server-side catalog actions. The tour composes the existing allowlisted action catalog.

Figma: none provided (terminal-rendered experience; copy and layout follow the existing `warpctrl tour` stop copy).

## Behavior

### Interactive runner: `warpctrl tour run`

1. Running `warpctrl tour run` in a terminal starts an interactive guided tour in that terminal. The runner requires a running, compatible Warp instance with Scripting enabled; when these preconditions fail, it prints the same enablement guidance the tour copy uses today (Settings > Scripting, build flags for local development) and exits non-zero.
2. If multiple compatible instances are running, the runner lists them and prompts the user to pick one by number. `--instance <id>` / `--pid <pid>` skip the prompt. With exactly one instance, no prompt appears.
3. The runner requires an interactive TTY on stdin/stdout. When stdin is not a TTY, it exits non-zero with a message explaining that `tour run` is interactive and pointing to `warpctrl tour <stop>` for scripted copy access.
4. On start, the runner prints the existing welcome banner, records the currently active window/tab/pane as the **anchor** (the pane the user ran the command in), and creates one reusable **tour pane** as a right-hand split of the anchor. All surface demonstrations open in the tour pane; the anchor keeps the runner's prompts visible at all times.
5. All prompts are numbered menus read line-by-line from stdin (e.g. `1) Start the core tour  2) Jump to a topic  3) Ask Warp's agent  4) I'm done`). Menus work under `TERM=dumb` and do not require raw-mode key handling.
6. The stop set, ordering, copy, and hands-on tasks match the existing tour: core stops (Themes, Keybindings, Panes & Panels, Global Search, Vertical Tabs) followed by optional topic groups (Terminal fundamentals, Coding workflow, Agents, Knowledge & navigation). Stop copy is the existing `warpctrl tour <stop>` copy, printed verbatim.
7. Before building any menu, the runner queries surface availability and omits stops whose destination surfaces are unavailable. If a surface open fails mid-tour (e.g. `unsupported_action`, `target_state_conflict`), the runner reports it in one friendly line, drops that stop from future menus, and continues — a single failed stop never aborts the tour.
8. Every stop prompt offers at minimum: mark the task done, get a one-line hint, skip the stop, ask Warp's agent (see 14–16), and end the tour. After a stop completes or is skipped, the runner offers: next stop, back to the topic menu, or end.
9. The Themes stop saves the user's full theme state (system-follow flag, light theme, dark theme, active theme) before opening the theme picker and restores it exactly when the stop ends, when the tour ends, and on interrupt (see 12).
10. The runner never submits terminal input on the user's behalf, never changes settings or permissions, never creates or edits Warp Drive items, and never closes anything that existed before the tour started.
11. Light environment-based personalization, with no model calls: when the working directory is inside a git repository, the runner references the repository by name in its framing and lists the Coding workflow topic first in the topic menu; outside a repository topics appear in their default order. Personalization never adds or removes stops beyond availability filtering.
12. Interrupt (Ctrl+C) or stdin EOF at any point triggers best-effort cleanup: restore saved theme state, print a summary of any tour-created panes/tabs still open, and exit. The runner never leaves the theme silently modified.
13. At tour end (completed, or user chose to end), the runner prints the existing cleanup copy and asks whether to close tour-created panes/tabs. If yes, it closes exactly the recorded tour-created IDs through normal Warp close behavior and tells the user that Warp's standard close confirmations may appear. If a close fails or is cancelled, the runner reports exactly what is still open. If no, it leaves them and says so.

### Agent escape hatch

14. Every menu includes an "Ask Warp's agent" option. Selecting it prompts the user for their question (one line of free text), then stages — but never submits — a prefilled prompt in an agent input: the runner creates an agent tab (or reuses the agent tab it created earlier in this tour run), inserts a prompt containing the user's question plus compact tour context (current stop, what is open in the tour pane, anchor/tour pane IDs), and tells the user to review and press Enter to send it.
15. The staged prompt is plain text the user can read and edit before submitting. Because the runner only stages input, no agent request is ever issued — and nothing is ever billed — without the user explicitly submitting it. This invariant must not regress.
16. After staging, the runner pauses with a "press Enter here to resume the tour" prompt in the anchor pane. Resuming returns to exactly the menu the user left. The agent tab created for the escape hatch counts as tour-created state and is offered for cleanup at the end (13), but is never closed while it has the user's unsubmitted or in-flight conversation without an explicit cleanup confirmation.

### Composite commands for the agent-driven tour

The consumer of these commands is an agent following the `warp-tour` skill. Their purpose is to collapse what is currently 4–6 CLI invocations (and the inference turns between them) per stop into one invocation.

17. `warpctrl tour init` performs tour startup in one invocation: selects the instance, records the active target chain as the anchor, queries surface availability, creates the tour pane as a right split of the anchor, and saves current theme state. With `--output-format json` it returns a single object containing: instance ID, anchor window/tab/pane/session IDs, the created tour pane ID, per-surface availability, and the saved theme state. If pane creation fails, `tour init` still returns the anchor and availability data with an explicit error entry for the pane step and exits non-zero.
18. `warpctrl tour stop <stop-name> --tour-pane <id> --anchor-pane <id>` performs one tour stop in one invocation: emits the stop's copy, opens the stop's destination surfaces in the tour pane, resolves the stop's relevant keybindings, and refocuses the anchor pane last. `<stop-name>` accepts the existing stop names (`themes`, `keybindings`, `panes`, `global-search`, `vertical-tabs`, `terminal`, `coding`, `agents`, `knowledge`). JSON output includes the copy text, a per-step result list (each surface open, keybinding lookup, and focus, with success or structured error), and resolved keybindings.
19. `tour stop` exits zero when the copy was emitted and the anchor was refocused, even if individual surface opens failed; the per-step results carry the failures so the agent can adjust without retrying the whole stop. It exits non-zero only when no meaningful work could be performed (no instance, invalid stop name, stale anchor).
20. `warpctrl tour finish --tour-pane <id> [--tour-tab <id> ...] [--restore-theme <saved-state-json>]` emits the existing cleanup copy, restores theme state when provided, closes exactly the given tour-created IDs through normal Warp close behavior, and returns per-step results. It never infers IDs to close; only explicitly passed IDs are touched. (`finish` rather than `cleanup` because `tour cleanup` already names the copy-printing command.)
21. The existing per-stop copy commands (`warpctrl tour welcome`, `warpctrl tour themes`, …, `warpctrl tour cleanup`) keep working unchanged so an updated skill can fall back to granular commands when pointed at an older CLI build.
22. The `warp-tour` skill is updated to use `tour init` / `tour stop` / `tour finish`, and to begin by offering the user the zero-cost path: if the user just wants the standard tour, the agent points them at `warpctrl tour run` (staging the command in their input, not running it) before offering to drive the tour itself.

### Cross-cutting invariants

23. Neither the runner nor the composite commands make any model calls or network requests beyond the existing loopback local-control transport. The full `tour run` experience works offline and costs the user nothing.
24. All tour functionality respects the existing security model: each underlying action is individually credentialed and allowlisted; the tour adds no new server-side actions and no new permission surface.
25. On platforms where local control is unavailable (e.g. Windows until broker transport lands), all tour commands fail with the existing structured no-instance/disabled errors rather than a bespoke failure mode.
