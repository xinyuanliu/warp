# Recording Finalization and Upload Reliability — Product Spec

## Summary
Guarantee that every computer-use video recording reaches a single, well-defined terminal outcome — published as an artifact, or explicitly discarded/failed — no matter how the run ends. Today a recording is only finalized when the agent explicitly calls `stop_recording`; any other exit (agent finishes without stopping, user cancels, ffmpeg hits a limit or crashes, the run tears down mid-upload) orphans the capture, leaves a temp file on disk, and loses the video.

## Problem
Finalization (stop ffmpeg gracefully → upload the artifact → report a result) lives entirely inside the `stop_recording` tool call. The live capture handle is held in a runtime-global controller that has no teardown hook, and the client's cancellation/finish paths only touch tool calls that are still mid-execution — an already-started recording is invisible to them. As a result there is no single place that owns "end this recording and publish it," so recordings are silently lost on the common non-happy-path exits.

## Goals
- **Core invariant:** every recording that reaches the live/capturing state produces exactly one terminal outcome — `Published`, `Discarded`, or `Failed` — and is never left running with no owner (invariant 4).
- All the ways a run can end — agent finish, user/agent cancel, new query, shell exit, capture limit, ffmpeg crash, process teardown — converge on the same finalization behavior.
- In-flight artifact uploads (recordings, and by generalization screenshots/files) are drained before the run's process is allowed to exit, within a bounded time budget.
- No temp capture files are left on disk after a recording terminates.

## Non-goals
- Recording-usage metrics/telemetry (separate change).
- Blocklist rendering of recording cards (feature #1).
- Burned-in click/keyboard overlays on the recorded video (feature #3). Finalization is only the hook point where burn-in will later run.
- Surviving a hard sandbox kill (max-instance-runtime TTL) that fires mid-capture. See invariant 25 and the fast-follow note.

## Behavior

### Lifecycle and terminal outcomes
1. A recording moves through the states `Idle → Starting → Active → Finalizing → {Published | Discarded | Failed}`. `Published`, `Discarded`, and `Failed` are terminal.
2. `Starting` is entered when `start_recording` reserves the single runtime slot; `Active` is entered once capture is confirmed live and a `recording_id` is issued to the agent. At most one recording may be `Starting` or `Active` per client runtime at a time; a second `start_recording` while one is in progress fails with an "already in progress" error and does not disturb the existing recording.
3. `Finalizing` is entered exactly once per recording, triggered by any finalization cause (see 8). Finalization is idempotent: repeated or concurrent triggers for the same recording perform the stop+upload work at most once and yield exactly one terminal outcome.
4. **A recording that reaches `Active` always eventually reaches a terminal outcome** — it can never be left running with no owner. This is the core invariant this feature exists to guarantee.
5. `Published`: the capture was finalized into a valid video, uploaded, and associated with the conversation as a viewable artifact. The outcome carries the artifact reference, capture metadata (duration, dimensions, size), a completion status (`Completed` vs `StoppedEarly`), and a human-readable `termination_reason`.
6. `Discarded`: no artifact is produced (e.g. capture never yielded a usable file, or was abandoned before any frames), the outcome carries a `termination_reason`, and no error is surfaced as a failure.
7. `Failed`: finalization was attempted but could not complete (e.g. ffmpeg could not finalize the container, or the upload failed after its retry budget). The outcome carries an error message.

### Finalization causes (every premature exit)
8. Finalization is triggered by exactly one of these causes, each mapped to a `termination_reason` on the terminal outcome:
   - `StoppedByAgent` — the agent called `stop_recording`. Happy path.
   - `AgentFinished` — the agent ended the run/turn (e.g. `finish_computer_use`) while a recording was still `Active`.
   - `LimitReached` — ffmpeg auto-stopped on the server-owned duration or size cap; completion status is `StoppedEarly`.
   - `FfmpegExited` — the capture process exited unexpectedly (crash/kill) before a stop was requested.
   - `Cancelled` — the recording's turn/conversation was cancelled or preempted (user cancel, new query submitted, agent-run shell exited, pane closed).
9. Happy path (`StoppedByAgent`): the agent calls `stop_recording` with the `recording_id`; capture is stopped gracefully so the video is playable; the video uploads and the outcome is `Published` with `termination_reason` "Stopped by agent" and completion status `Completed`.
10. `AgentFinished`: if the agent finishes without stopping an `Active` recording, the recording is finalized automatically as part of run teardown — capture is stopped gracefully, the video uploads, and the artifact is published and associated with the conversation. The run's own success/failure is never changed by recording finalization.
11. `LimitReached`: when ffmpeg auto-stops at the duration or size cap, the recording is finalized proactively (without waiting for a later `stop_recording`) and the video is published immediately with completion status `StoppedEarly` and a `termination_reason` indicating the cap was hit.
    - The agent is not interrupted mid-turn. It learns the recording has already completed the next time it references it: a later `stop_recording` (or `finish_computer_use`) for that `recording_id` returns a benign completion/no-op result carrying `StoppedEarly` and the cap `termination_reason`, never an error implying data loss.
    - **Open question:** should the client also proactively surface a notice into the agent's context at limit time — so a long computer-use sequence stops acting as if capture is still running — rather than only informing it on the next stop/finish? There is no live recording tool call to attach a delayed result to, so proactive push requires a server-emitted message; see the tech spec. Default is the next-reference notification above.
12. `FfmpegExited`: if the capture process exits unexpectedly, the recording is finalized. If a usable file exists it is published (`StoppedEarly`); otherwise the outcome is `Discarded` or `Failed` with an explanatory reason. No orphaned process or handle remains.
13. `Cancelled`: when the owning turn/conversation is cancelled or preempted, the recording is finalized rather than orphaned. Finalization makes a best-effort attempt to stop gracefully and publish what was captured; if that is not possible the outcome is `Discarded`. The terminal outcome carries a `termination_reason` that identifies the cancellation.

### Cancellation, start/stop edge cases
14. Start canceled mid-flight: if the turn is cancelled while `start_recording` is still bringing capture up, the runtime slot is released, no artifact is produced, no `recording_id` is left registered, and the temp capture file (if any was created) is removed. The outcome is `Discarded`.
15. Stop canceled mid-upload: if `stop_recording`'s upload is interrupted by cancellation, the upload is not abandoned — it is allowed to complete so the artifact is still published, and the terminal outcome (with artifact reference) is still surfaced rather than silently dropped.
16. The terminal outcome for a recording is produced independently of whether the triggering tool call is still tracked as "executing." Cancelling or losing the tool call must not suppress the recording's terminal outcome.
17. Conversation-not-synced at stop time (defensive guard; effectively unreachable in the cloud computer-use flow, because recording tool calls are server-emitted and therefore only issued after the conversation already has a server identity): if `stop_recording` ever runs without a server conversation token, it returns a distinct, non-fatal "conversation not synced" result and leaves the recording `Active` so a retry can succeed; it never discards the capture. **Open question:** keep this guard as cheap insurance or drop it — see the tech spec recommendation (keep).
18. Upload failure at stop time: if the upload exhausts its retry budget, the outcome is `Failed` with the upload error. (Whether the local file is retained for a later retry is a tech decision; from the agent's perspective the result is `Failed`.)

### Resource and teardown invariants
19. On any terminal outcome (`Published`, `Discarded`, or `Failed`), the temporary capture file and its sidecar log are removed from disk. No terminal path leaves capture temp files behind.
20. Capture is always bounded: even a recording that is never explicitly stopped cannot grow without bound or run forever, because the server-owned duration and size caps stop ffmpeg. An orphaned capture self-terminates and is then finalized via `FfmpegExited`/`LimitReached`.
21. Teardown drain: when a run ends, the process does not exit until in-flight artifact uploads have completed or a bounded upload-time budget elapses. This drain is generalized across artifact types (recordings, screenshots, uploaded files), so no artifact type is silently lost to teardown. If the budget elapses, teardown proceeds and the affected outcome is `Failed`.
22. The drain budget is bounded so a stuck upload cannot wedge run teardown indefinitely.

### Scoping and agent guidance
23. A recording is scoped to a single user query's response. Within that response it may span many agent/tool turns — started in one computer-use tool call and stopped in a later turn of the same computer-use subagent — because finalization is evaluated against the whole runtime, not a single turn, so a recording is never dropped merely because its starting turn ended. It never bleeds across user queries: submitting a new user query cancels the in-flight response and finalizes the active recording under cause `Cancelled` (per 13), so the next query always begins with no active recording. ("A later resumed turn" means an auto-resume or continuation within the same response, not a new user query.)
24. Cancellation guidance: when a recording is finalized under `Cancelled`, the agent is informed via the `termination_reason` that the recording was ended by interruption, and — on resumption — is instructed to start a new recording if it still needs one. A cancelled recording is never silently resumed under the old `recording_id`.
25. Known limitation — hard runtime kill (distinct from graceful teardown):
    - Graceful, client-controlled teardown (agent finish, idle-timeout expiry): the run's own shutdown finalizes any still-`Active` recording (graceful stop → valid video) and blocks on the upload drain before the process exits, so nothing is lost (invariants 10, 21).
    - Hard runtime kill (max-instance-runtime TTL or external/infra kill): the whole runtime is destroyed abruptly with no client grace period, so finalization never runs. A recording still `Active` at that instant is a total loss the drain cannot recover, because (i) capture was never gracefully stopped, so the on-disk file is truncated/unplayable, and (ii) nothing has been uploaded yet (the video uploads only once, at finalization). This is out of scope here; the fast-follow is to stream/segment the upload during capture so earlier segments are already uploaded, bounding loss to the last un-flushed segment. See the tech spec.

### Consistency
26. All finalization causes converge on identical stop+publish behavior and identical terminal-outcome shape; the only differences between causes are the `termination_reason`/completion status and whether a usable file existed. There is no cause-specific divergence in whether temp files are cleaned, whether the artifact is associated with the conversation, or whether a terminal outcome is produced.
