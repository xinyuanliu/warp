# Local child event-stream sharing — Optimized Tech Spec
## Context
QUALITY-790 moved owner-side parent orchestrators away from explicit `run_ids[]` streams and toward a parent-family ancestor stream: `ancestor_run_id=<parent>&include_self=true`. That stream contains events for the parent run and all direct child runs, which means it already carries the events local children currently subscribe to with their own `RunIds(self)` streams.
Current relevant code:
- `app/src/ai/blocklist/orchestration_event_streamer.rs` owns event delivery for local conversations. `register_consumer` records active consumers, `register_watched_run_id` records child run IDs on the parent, and `start_sse_connection` opens an owner-side SSE for each eligible conversation.
- `app/src/ai/blocklist/orchestration_event_streamer.rs` selects an owner-side `AncestorRunId { include_self: true }` filter when a conversation is a parent and `FeatureFlag::OwnerOrchestrationAncestorStreamer` is enabled. Child-only conversations still select `RunIds([self_run_id])`.
- `app/src/ai/blocklist/action_model/execute/start_agent.rs` completes child startup by calling `OrchestrationEventStreamer::register_watched_run_id(parent_conversation_id, child_run_id, ctx)`.
- `app/src/ai/blocklist/orchestration_events.rs` queues hydrated message and lifecycle inputs per target conversation and emits `EventsReady` for that target.
- `app/src/ai/agent_events/driver.rs` treats an SSE as a source of `AgentRunEvent`s and advances a single stream cursor.
Today, when a local parent and its local child both have active consumers, they can hold overlapping streams:
- Parent: `AncestorRunId { ancestor_run_id: parent, include_self: true }`, which includes child events.
- Child: `RunIds([child])`, used for the child's inbox and lifecycle events.
This is redundant for direct local children while the parent stream is alive.
## Proposed changes
### Chosen approach: parent stream demux for local direct children
Use the parent-family stream as the single network stream for the parent plus its direct local children whenever the parent stream is active. The parent stream will demultiplex events by `event.run_id`:
- `event.run_id == parent_run_id`
  - Hydrate parent-addressed `new_message` events for the parent conversation.
  - Ignore parent self lifecycle events for orchestration injection, same as today.
- `event.run_id == local_child_run_id`
  - Hydrate `new_message` events for the local child conversation.
  - Convert child lifecycle events into parent-facing lifecycle events and enqueue them for the parent conversation.
  - Advance the child's local cursor so the child does not replay the same event if it later opens its own stream.
- `event.run_id == remote_child_run_id`
  - Keep current parent-facing lifecycle behavior.
  - Do not hydrate child-addressed messages; the remote child process owns that inbox.
This keeps parent observability and local child inbox delivery on one SSE without requiring a server change.
### Parent-covered child state
Add a small routing table to `OrchestrationEventStreamer`, maintained from `BlocklistAIHistoryModel`:
- `local_child_by_run_id: HashMap<String, AIConversationId>` or equivalent lookup helper.
- A local child is eligible for parent-stream routing when:
  - it has a `run_id`,
  - it has `has_parent_agent()`,
  - it is not `is_remote_child()`,
  - it is not a passive shared-session viewer,
  - its direct parent conversation is present locally and that parent has an active parent-family ancestor stream.
The implementation should avoid storing duplicative permanent state if a lookup through `BlocklistAIHistoryModel` is cheap enough. A helper such as `local_child_conversation_for_run_id(run_id, parent_conversation_id, ctx)` may be preferable to a new long-lived map if it can use existing indexes.
### Stream selection changes
Keep the existing owner-side decision table for parent conversations. Change child-only selection to ask whether the child is covered by an active local parent stream:
- If the child is covered by a parent-family stream, do not open a child `RunIds(self)` SSE.
- If the parent stream is absent, closed, ineligible, or not parent-family, keep the child `RunIds(self)` SSE as fallback.
- If the parent stream later appears, tear down the redundant child stream after draining buffered events.
- If the parent stream later disappears while the child still has an active consumer, reopen the child `RunIds(self)` stream from the child's persisted cursor.
This preserves standalone child behavior and keeps the optimization scoped to cases where the parent can actually cover the child.
### Demux and cursor semantics
The main subtlety is cursor ownership.
The parent-family stream has one network cursor: the max sequence delivered by that stream. When the stream sees a child event and routes it to the child conversation, update both:
- the parent stream cursor, because the event was consumed by the parent-family stream;
- the child conversation cursor, because the event has been delivered to that local child through the parent stream.
This avoids replay if the child later falls back to its own `RunIds(self)` stream.
Child cursor persistence should reuse the existing owner-side `persist_event_cursor` path for the child conversation. That preserves the behavior the child would have had with its own `RunIds(self)` stream, including server-side cursor updates for the child run where the existing path already performs them.
### Message hydration
The current `SseForwardingConsumer` hydrates `new_message` events only for its `self_run_id`. Parent demux needs a hydrator per recipient run ID:
- parent run ID -> parent hydrator;
- local child run ID -> child hydrator;
- remote child run ID -> no child hydration.
Implementation options:
1. Extend `SseForwardingConsumer` to emit raw events only, and hydrate in `OrchestrationEventStreamer` after choosing the target conversation.
2. Keep hydration in the consumer but give it a resolver callback from run ID to target conversation/hydrator.
Prefer option 1 if feasible. It keeps target selection, cursor updates, and event queueing in the model that owns conversation state, and avoids giving the background driver task direct access to mutable app state.
### Event queueing
For a single parent-family event batch, partition events into target batches:
- Parent batch:
  - parent-addressed messages;
  - lifecycle events from any direct child.
- Child batches:
  - child-addressed messages for local children;
  - possibly child self lifecycle only if the child needs to observe its own lifecycle locally. If no current behavior depends on child self lifecycle injection, do not add it.
Then call `OrchestrationEventService::enqueue_event_batch` once per target conversation with its pending events.
### Restore behavior
On restore:
- Parent conversations continue to fetch server task metadata and children, then open parent-family streams when applicable.
- Local child conversations should initially avoid opening `RunIds(self)` only if their parent stream is already active or can be opened immediately.
- If restore order means the child appears before the parent has opened a stream, the child can temporarily open `RunIds(self)`. Once the parent stream opens, the stale-filter path should tear down the redundant child stream.
The design should prefer correctness over avoiding a brief duplicate stream during restore.
## Rejected alternatives
### Always use ancestor streaming for child-only conversations
A child-only conversation could use `ancestor_run_id=<child>&include_self=true`, but that is more expensive than `RunIds(self)` and only becomes useful if the child later becomes a parent. Keeping child-only on `RunIds(self)` remains simpler unless a parent stream can cover it.
### Let children rely on parent stream without fallback
This would reduce streams but breaks when the parent view closes, the parent is not active, or the child is restored independently. Local child `RunIds(self)` must remain a fallback.
### Route remote child inboxes through the parent stream
Remote child runs are executed by another process, which owns their inbox and wake behavior. Parent-side demux should not hydrate or deliver remote child messages locally.
### Share the viewer-mode ancestor stream
Viewer-mode streams intentionally drop `new_message` events and only update pill-bar status. They are not suitable for owner-side or child inbox delivery.
## Testing and validation
Add focused unit tests in `app/src/ai/blocklist/orchestration_event_streamer_tests.rs`:
- Parent-family stream covers a local direct child: child does not open a `RunIds(self)` stream while the parent stream is active.
- Parent-family stream delivers a child-addressed `new_message` to the local child conversation.
- Parent-family stream still delivers child lifecycle events to the parent conversation.
- Remote child under the same parent is not hydrated locally.
- If the parent stream closes while the local child has an active consumer, the child opens `RunIds(self)` from the child cursor.
- If a child `RunIds(self)` stream is open first and the parent-family stream later opens, the child stream tears down after draining.
- Child cursor advances when the parent stream routes a child-addressed event to the child.
Run:
- `./script/format`
- focused `cargo test -p warp --lib orchestration_event_streamer`
- `cargo clippy --workspace --all-targets --all-features --tests -- -D warnings` before PR update.
Manual validation:
1. Start a local parent agent that launches a local child.
2. Confirm only the parent-family SSE remains open while both conversations are active.
3. Send a parent-to-child message and confirm the child receives it.
4. Close or deactivate the parent view while the child remains active and confirm the child falls back to a self run-id stream.
## Risks and mitigations
- **Cursor skew between parent and child**: update the child local cursor when routing child events through the parent stream.
- **Dropped child messages when parent closes**: keep `RunIds(self)` fallback and test parent teardown.
- **Duplicate delivery during handoff**: drain before tearing down redundant child streams and dedupe by sequence.
- **Remote child ownership confusion**: gate child-message demux to local children only.
- **Nested orchestration**: direct parent-family stream only covers direct children. A child that becomes a parent may still need its own parent-family stream for its children.
## Parallelization
This implementation is mostly in one client subsystem, so broad parallelization is not useful. A practical split is:
- **streamer-implementation**: local agent in `/Users/matthew/src/too-many-agents/warp` on branch `matthew/quality-772-optimized`, owning `orchestration_event_streamer.rs` and tests.
- **review-validation**: separate local agent after implementation, reviewing cursor/hydration/fallback behavior against this spec.
Do not run both implementation agents on the same worktree. If parallel work is needed, create a separate worktree under `/Users/matthew/src/too-many-agents/`.
