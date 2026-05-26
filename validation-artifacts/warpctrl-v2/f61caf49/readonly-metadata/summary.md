# WarpCtrl readonly metadata validation summary
Validated SHA: `f61caf49400dc5c0d37d57a553d27733700e5204`
Artifact root: `validation-artifacts/warpctrl-v2/f61caf49/readonly-metadata`

## Counts
- Pass: 13
- Fail: 0
- Skip: 0

## Visual inspection
- Failures/blockers: none
- Combined screenshot evidence captured for every executed command in `screenshots/`.
- Visual comparison used one visible Warp window with one visible `~` tab, one visible terminal pane, and one active terminal session. Opaque IDs are not rendered in Warp UI, so validation compared visible counts, active state, title labels, and target presence against JSON metadata.

## Selector edge-case behavior
- `window inspect --window missing-window-id`: exited 1 with JSON `stale_target` (`window.list cannot resolve the requested window id`).
- `tab inspect --tab-index 999`: exited 1 with JSON `stale_target` (`tab.list cannot resolve the requested tab index`).
- `window inspect --window active --window-index 0`: exited 2 with clap conflict error before dispatch.

## Blockers
None.

## Skipped commands
None.

## Notes
The graphical validation run used the existing `skip_firebase_anonymous_user` Cargo feature in addition to `gui,warp_control_cli` so the isolated profile could bypass login/onboarding and expose a terminal workspace for visual metadata comparison. A baseline `gui,warp_control_cli` app build also passed and is logged.
