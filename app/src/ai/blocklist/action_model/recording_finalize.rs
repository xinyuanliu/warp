//! Centralized recording finalization: the single place that stops capture,
//! uploads the video, cleans up temp files, and reports a terminal outcome.
//!
//! Every exit path converges here via [`finalize_recording`] (the `stop_recording`
//! tool call, the cancellation funnel, the ffmpeg-exit watcher, and the driver
//! run-tail drain), so a recording that reached the live state always reaches a
//! single terminal outcome. Finalization is idempotent by construction: the live
//! handle is `Option`-taken from [`RecordingController`], so whoever claims it
//! first runs the stop+upload and everyone else no-ops.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use warpui::r#async::Timer;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use super::artifact_upload_state::{ArtifactUploadGuard, ArtifactUploadState};
use super::recording_controller::{FinalizeReason, RecordingController};
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent_sdk::artifact_upload::{
    CompletedFileArtifactUpload, FileArtifactUploadRequest, FileArtifactUploader,
};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::server::server_api::ai::AIClient;
use crate::server::server_api::{ServerApi, ServerApiProvider};

/// How often the watcher polls a live recording for an early capture exit.
const EXIT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The terminal outcome of finalizing a recording. Surfaced to the agent as a
/// `StopRecording` result on the agent-driven path, and logged (never silently
/// dropped) on every path so the outcome is visible even off the action map.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Clone)]
pub enum RecordingTerminalOutcome {
    /// Capture was finalized into a video, uploaded, and associated with the
    /// conversation as an artifact.
    Published {
        artifact_uid: String,
        duration: Duration,
        width_px: i32,
        height_px: i32,
        size_bytes: i64,
        completion_status: computer_use::RecordingCompletionStatus,
        termination_reason: String,
    },
    /// No artifact was produced (nothing usable was captured); not surfaced as an
    /// error.
    Discarded { termination_reason: String },
    /// Finalization was attempted but could not complete.
    Failed { error: String },
}

impl RecordingTerminalOutcome {
    /// Surfaces the outcome independent of the action map, so a finalize triggered
    /// without a live tool call (watcher, cancel, run-tail) is never silently lost.
    fn log(&self, reason: FinalizeReason) {
        match self {
            RecordingTerminalOutcome::Published { artifact_uid, .. } => {
                log::info!("Recording finalized ({reason:?}): published artifact {artifact_uid}");
            }
            RecordingTerminalOutcome::Discarded { termination_reason } => {
                log::info!("Recording finalized ({reason:?}): discarded ({termination_reason})");
            }
            RecordingTerminalOutcome::Failed { error } => {
                log::warn!("Recording finalized ({reason:?}): failed ({error})");
            }
        }
    }
}

/// Uploads a finalized recording file as a conversation artifact. Abstracted so
/// tests can inject a mock without a live server or HTTP stack.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[async_trait]
pub trait RecordingUploader: Send + Sync {
    async fn upload(
        &self,
        request: FileArtifactUploadRequest,
    ) -> anyhow::Result<CompletedFileArtifactUpload>;
}

/// Production [`RecordingUploader`] backed by [`FileArtifactUploader`].
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub struct RealRecordingUploader {
    uploader: FileArtifactUploader,
}

#[cfg_attr(target_family = "wasm", allow(dead_code))]
impl RealRecordingUploader {
    pub fn new(
        ai_client: Arc<dyn crate::server::server_api::ai::AIClient>,
        server_api: Arc<crate::server::server_api::ServerApi>,
    ) -> Self {
        Self {
            uploader: FileArtifactUploader::new(ai_client, server_api),
        }
    }
}

#[async_trait]
impl RecordingUploader for RealRecordingUploader {
    async fn upload(
        &self,
        request: FileArtifactUploadRequest,
    ) -> anyhow::Result<CompletedFileArtifactUpload> {
        let association = self.uploader.resolve_upload_association(&request).await?;
        self.uploader
            .upload_with_association(request, association)
            .await
    }
}

/// Stops capture gracefully, runs the (future) burn-in hook, uploads the video,
/// removes temp files on every terminal branch, and returns the terminal outcome.
///
/// `_upload_guard` is held for the duration so the driver run-tail drain observes
/// this upload as in-flight; it is acquired synchronously by the caller.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub async fn finalize_recording(
    recorder: Box<dyn computer_use::Recorder>,
    uploader: Arc<dyn RecordingUploader>,
    _upload_guard: ArtifactUploadGuard,
    handle: computer_use::RecordingHandle,
    actions: Vec<computer_use::ActionLogEntry>,
    reason: FinalizeReason,
    server_conversation_token: Option<ServerConversationToken>,
) -> RecordingTerminalOutcome {
    let output = match recorder.stop(handle).await {
        Ok(output) => output,
        Err(error) => {
            // Best-effort paths (interrupted / crashed) discard when nothing
            // usable was captured; explicit stop/finish surface a failure. The
            // recorder cleans up its own temp file on its error branches.
            let outcome = match reason {
                FinalizeReason::Cancelled | FinalizeReason::FfmpegExited => {
                    RecordingTerminalOutcome::Discarded {
                        termination_reason: reason.termination_reason(
                            computer_use::RecordingCompletionStatus::StoppedEarly,
                        ),
                    }
                }
                _ => RecordingTerminalOutcome::Failed {
                    error: error.to_string(),
                },
            };
            outcome.log(reason);
            return outcome;
        }
    };

    let local_path = output.path.clone();
    let log_path = local_path.with_extension("log");

    // Burn keyboard action pills into the video before upload. Best-effort: on
    // any failure the original capture is uploaded unannotated (a no-labels video
    // beats no video). The overlay file, when produced, is a sibling of the mp4.
    let mut upload_path = local_path.clone();
    let mut overlay_path: Option<std::path::PathBuf> = None;
    if !actions.is_empty() {
        match computer_use::burn_in_action_log(&local_path, &actions, (output.width, output.height))
            .await
        {
            Ok(path) if path != local_path => {
                overlay_path = Some(path.clone());
                upload_path = path;
            }
            Ok(_) => {}
            Err(error) => {
                log::warn!("Recording overlay burn-in failed; uploading original: {error}");
            }
        }
    }

    let request = FileArtifactUploadRequest {
        path: upload_path,
        run_id: None,
        conversation_id: server_conversation_token,
        description: None,
    };
    let upload_result = uploader.upload(request).await;

    // Invariant: remove the temp mp4 + sidecar log (and the overlay, if any) on
    // every terminal branch.
    // TODO(recording upload retry, follow-on): on upload failure, retain the
    // local file and retry via the upload registry instead of discarding it.
    let _ = std::fs::remove_file(&local_path);
    let _ = std::fs::remove_file(&log_path);
    if let Some(overlay_path) = overlay_path.as_ref() {
        let _ = std::fs::remove_file(overlay_path);
    }

    let outcome = match upload_result {
        Ok(upload) => RecordingTerminalOutcome::Published {
            artifact_uid: upload.artifact.artifact_uid,
            duration: output.duration,
            width_px: output.width as i32,
            height_px: output.height as i32,
            size_bytes: output.size_bytes as i64,
            completion_status: output.completion_status,
            termination_reason: reason.termination_reason(output.completion_status),
        },
        Err(err) => RecordingTerminalOutcome::Failed {
            error: format!("Recording upload failed: {err:#}"),
        },
    };
    outcome.log(reason);
    outcome
}

/// Resolves the finalize dependencies (server conversation token + server
/// clients) from a concrete app context. Split out from [`spawn_detached_finalize`]
/// because `SingletonEntity::as_ref` takes `&AppContext`, which only coerces from a
/// concrete `ModelContext`, not a generic one.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub fn recording_finalize_deps(
    ctx: &AppContext,
    conversation_id: AIConversationId,
) -> (
    Option<ServerConversationToken>,
    Arc<dyn AIClient>,
    Arc<ServerApi>,
) {
    let token = BlocklistAIHistoryModel::as_ref(ctx)
        .conversation(&conversation_id)
        .and_then(|conversation| conversation.server_conversation_token())
        .cloned();
    let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client();
    let server_api = ServerApiProvider::as_ref(ctx).get();
    (token, ai_client, server_api)
}

/// Spawns a detached finalize for an already-claimed recording handle. Used by the
/// cancellation funnel, the exit watcher, and the driver run-tail — none of which
/// have a live tool call to attach a result to, so the outcome is logged by
/// `finalize_recording`. Resolve deps with [`recording_finalize_deps`] first.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub fn spawn_detached_finalize<T: Entity>(
    ctx: &mut ModelContext<T>,
    handle: computer_use::RecordingHandle,
    actions: Vec<computer_use::ActionLogEntry>,
    reason: FinalizeReason,
    token: Option<ServerConversationToken>,
    ai_client: Arc<dyn AIClient>,
    server_api: Arc<ServerApi>,
) {
    let uploader: Arc<dyn RecordingUploader> =
        Arc::new(RealRecordingUploader::new(ai_client, server_api));
    let guard = ArtifactUploadState::global().begin();
    let recorder = computer_use::create_recorder();
    ctx.spawn(
        finalize_recording(recorder, uploader, guard, handle, actions, reason, token),
        |_model, _outcome, _ctx| {},
    );
}

/// Spawns a recurring poll that finalizes the recording when its capture process
/// exits on its own (duration/size cap or crash), modeled on the long-running
/// command block watcher. Re-arms itself while the recording is still active and
/// stops once the recording is finalized by any path.
///
/// TODO(recording proactive notify, follow-on): on early exit the agent isn't
/// notified mid-turn; it learns on its next `stop_recording`/`finish` (2a). A
/// proactive notice would reuse `OrchestrationEventService`'s per-conversation
/// input channel (`MessagesReceivedFromAgents`/`EventsFromAgents`); recording is
/// single-per-runtime, so it may not need that channel's full multiplexing.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub fn spawn_recording_exit_watcher(
    recording_id: String,
    ctx: &mut ModelContext<RecordingController>,
) {
    ctx.spawn(
        async move {
            Timer::after(EXIT_POLL_INTERVAL).await;
        },
        move |controller, _output, ctx| match controller.poll_active_exit(&recording_id) {
            Some(kind) => {
                if let Some((conversation_id, handle, actions)) =
                    controller.take_by_id(&recording_id)
                {
                    let reason = match kind {
                        computer_use::RecordingExitKind::LimitReached => {
                            FinalizeReason::LimitReached
                        }
                        computer_use::RecordingExitKind::Crashed => FinalizeReason::FfmpegExited,
                    };
                    let (token, ai_client, server_api) =
                        recording_finalize_deps(ctx, conversation_id);
                    spawn_detached_finalize(
                        ctx, handle, actions, reason, token, ai_client, server_api,
                    );
                }
            }
            None => {
                // Still capturing: re-arm. If the recording is gone (stopped or
                // finalized elsewhere), stop watching.
                if controller.active_recording_id().as_deref() == Some(recording_id.as_str()) {
                    spawn_recording_exit_watcher(recording_id, ctx);
                }
            }
        },
    );
}

#[cfg(all(test, not(target_os = "linux")))]
#[path = "recording_finalize_tests.rs"]
mod tests;
