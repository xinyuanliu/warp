#[cfg(not(target_family = "wasm"))]
use std::sync::Arc;

#[cfg(not(target_family = "wasm"))]
use ai::agent::action_result::{RecordingStopped, StopRecordingResult};
use futures::future::BoxFuture;
use futures::FutureExt;
#[cfg(not(target_family = "wasm"))]
use warpui::SingletonEntity;
use warpui::{Entity, ModelContext};

use super::{ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput};
use crate::ai::agent::AIAgentActionType;
#[cfg(not(target_family = "wasm"))]
use crate::{
    ai::{
        agent::AIAgentActionResultType,
        blocklist::action_model::artifact_upload_state::ArtifactUploadState,
        blocklist::action_model::recording_controller::{
            FinalizeReason, RecordingController, StopRecordingControllerError,
        },
        blocklist::action_model::recording_finalize::{
            finalize_recording, RealRecordingUploader, RecordingTerminalOutcome, RecordingUploader,
        },
        blocklist::BlocklistAIHistoryModel,
    },
    server::server_api::ServerApiProvider,
};

/// Maps a finalize outcome to the `stop_recording` tool-call result.
#[cfg(not(target_family = "wasm"))]
fn stop_recording_result_from_outcome(outcome: RecordingTerminalOutcome) -> StopRecordingResult {
    match outcome {
        RecordingTerminalOutcome::Published {
            artifact_uid,
            duration,
            width_px,
            height_px,
            size_bytes,
            completion_status,
            termination_reason,
        } => StopRecordingResult::Success(RecordingStopped {
            artifact_uid,
            duration,
            width_px,
            height_px,
            size_bytes,
            completion_status,
            termination_reason,
        }),
        RecordingTerminalOutcome::Discarded { termination_reason } => {
            StopRecordingResult::Error(termination_reason)
        }
        RecordingTerminalOutcome::Failed { error } => StopRecordingResult::Error(error),
    }
}

pub struct StopRecordingExecutor;

impl StopRecordingExecutor {
    pub fn new() -> Self {
        Self
    }

    #[cfg_attr(target_family = "wasm", allow(unused_variables))]
    pub(super) fn should_autoexecute(
        &self,
        input: ExecuteActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> bool {
        let ExecuteActionInput { action, .. } = input;
        matches!(action.action, AIAgentActionType::StopRecording { .. })
            && warp_core::features::FeatureFlag::VideoRecording.is_enabled()
    }

    #[cfg_attr(target_family = "wasm", allow(unused_variables))]
    pub(super) fn execute(
        &mut self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> AnyActionExecution {
        #[cfg(target_family = "wasm")]
        {
            ActionExecution::<()>::InvalidAction.into()
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let ExecuteActionInput {
                action,
                conversation_id,
            } = input;
            let AIAgentActionType::StopRecording { recording_id } = &action.action else {
                return ActionExecution::<()>::InvalidAction.into();
            };
            let server_conversation_token = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&conversation_id)
                .and_then(|conversation| conversation.server_conversation_token())
                .cloned();
            let Some(server_conversation_token) = server_conversation_token else {
                return ActionExecution::<()>::Sync(AIAgentActionResultType::StopRecording(
                    StopRecordingResult::Error(
                        StopRecordingControllerError::ConversationNotSynced.to_string(),
                    ),
                ))
                .into();
            };

            let claimed = RecordingController::handle(ctx).update(ctx, |controller, _| {
                controller.take_handle_or_err(recording_id)
            });
            let (handle, actions) = match claimed {
                Ok(claimed) => claimed,
                Err(error) => {
                    return ActionExecution::<()>::Sync(AIAgentActionResultType::StopRecording(
                        StopRecordingResult::Error(error.to_string()),
                    ))
                    .into();
                }
            };

            let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client();
            let server_api = ServerApiProvider::as_ref(ctx).get();
            let uploader: Arc<dyn RecordingUploader> =
                Arc::new(RealRecordingUploader::new(ai_client, server_api));
            // Register the upload as in-flight synchronously so a concurrent run-tail
            // drain waits for it even if this action is cancelled mid-upload.
            let upload_guard = ArtifactUploadState::global().begin();
            let recorder = computer_use::create_recorder();

            ActionExecution::new_async(
                async move {
                    let outcome = finalize_recording(
                        recorder,
                        uploader,
                        upload_guard,
                        handle,
                        actions,
                        FinalizeReason::StoppedByAgent,
                        Some(server_conversation_token),
                    )
                    .await;
                    stop_recording_result_from_outcome(outcome)
                },
                |result, _ctx| AIAgentActionResultType::StopRecording(result),
            )
            .into()
        }
    }

    pub(super) fn preprocess_action(
        &mut self,
        _input: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }
}

impl Entity for StopRecordingExecutor {
    type Event = ();
}
