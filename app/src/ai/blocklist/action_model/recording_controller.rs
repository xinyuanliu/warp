//! Runtime-global registry of in-progress video recordings.
//!
//! A recording's capture process must outlive the `StartRecording` tool call
//! that launches it and survive until a later `StopRecording` call (possibly
//! from a later resumed turn), so the live handle lives here rather than in a
//! per-call executor.

use computer_use::{
    ActionLogEntry, RecordingCompletionStatus, RecordingExitKind, DEFAULT_PILL_DURATION,
};
use instant::Instant;
use thiserror::Error;
use warpui::{Entity, SingletonEntity};

use crate::ai::agent::conversation::AIConversationId;

#[derive(Debug, Error)]
pub enum StartRecordingControllerError {
    #[error("A recording is already in progress in this runtime.")]
    AlreadyInProgress,
}

#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Error)]
pub enum StopRecordingControllerError {
    #[error("No active recording with id '{recording_id}'.")]
    NoActiveRecording { recording_id: String },
    #[error("Current conversation has not been synced to the server yet.")]
    ConversationNotSynced,
}

/// Why a recording is being finalized. Determines the human-readable
/// `termination_reason` surfaced on the terminal result.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FinalizeReason {
    /// The agent explicitly called `stop_recording`.
    StoppedByAgent,
    /// The agent ended the run/turn while a recording was still active.
    AgentFinished,
    /// ffmpeg auto-stopped at the configured duration or size cap.
    LimitReached,
    /// The capture process exited unexpectedly before a stop was requested.
    FfmpegExited,
    /// The owning turn/conversation was cancelled or preempted.
    Cancelled,
}

#[cfg_attr(target_family = "wasm", allow(dead_code))]
impl FinalizeReason {
    /// The human-readable reason recorded on the terminal result, factoring in
    /// whether capture was still live when finalization ran.
    pub fn termination_reason(self, completion: RecordingCompletionStatus) -> String {
        match self {
            FinalizeReason::StoppedByAgent => match completion {
                RecordingCompletionStatus::Completed => "Stopped by agent".to_string(),
                RecordingCompletionStatus::StoppedEarly => {
                    "Recording stopped before the agent requested it".to_string()
                }
            },
            FinalizeReason::AgentFinished => {
                "Finalized because the agent finished without stopping the recording".to_string()
            }
            FinalizeReason::LimitReached => {
                "Stopped at the configured duration or size limit".to_string()
            }
            FinalizeReason::FfmpegExited => {
                "Capture process exited before the recording was stopped".to_string()
            }
            FinalizeReason::Cancelled => {
                "Recording was interrupted; start a new recording if one is still needed"
                    .to_string()
            }
        }
    }
}

/// The single in-progress recording: controller id, owning conversation, live
/// capture handle, and the action overlay log accumulated while it records.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
struct ActiveRecording {
    id: String,
    conversation_id: AIConversationId,
    handle: computer_use::RecordingHandle,
    /// When capture went live; action offsets are measured from here.
    started_at: Instant,
    /// Action groups to burn into the video, in dispatch order.
    actions: Vec<ActionLogEntry>,
}

/// Enforces a single active recording per client runtime and owns the
/// Idle -> Starting -> Active portion of the recording lifecycle. Whoever takes
/// the live handle first (stop, cancel, watcher, or the run-tail drain) wins; the
/// handle is `Option`-taken so finalization is idempotent by construction.
pub struct RecordingController {
    active: Option<ActiveRecording>,
    /// Set while a start is in flight (after reservation, before the recording is
    /// registered) so a concurrent start cannot race past the single-slot guard.
    starting: bool,
}

impl RecordingController {
    pub fn new() -> Self {
        Self {
            active: None,
            starting: false,
        }
    }

    /// Reserves the single recording slot, failing if one is already active or
    /// starting.
    pub fn try_begin_start(&mut self) -> Result<(), StartRecordingControllerError> {
        if self.starting || self.active.is_some() {
            return Err(StartRecordingControllerError::AlreadyInProgress);
        }
        self.starting = true;
        Ok(())
    }

    /// Registers a successfully started recording, releasing the start reservation.
    pub fn finish_start(
        &mut self,
        recording_id: String,
        conversation_id: AIConversationId,
        handle: computer_use::RecordingHandle,
    ) {
        self.starting = false;
        self.active = Some(ActiveRecording {
            id: recording_id,
            conversation_id,
            handle,
            started_at: Instant::now(),
            actions: Vec::new(),
        });
    }

    /// Releases the start reservation after a failed start.
    pub fn abort_start(&mut self) {
        self.starting = false;
    }

    /// Appends an overlay group to the active recording.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn record_action(&mut self, labels: Vec<String>) {
        if !labels.is_empty() {
            if let Some(active) = self.active.as_mut() {
                active.actions.push(ActionLogEntry {
                    offset: active.started_at.elapsed(),
                    labels,
                    show_duration: DEFAULT_PILL_DURATION,
                });
            }
        }
    }

    /// Removes and returns the live handle for `recording_id` (the agent-driven
    /// stop path), erroring if the active recording doesn't match. Taking the
    /// handle here makes a later finalize from another path a no-op.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn take_handle_or_err(
        &mut self,
        recording_id: &str,
    ) -> Result<(computer_use::RecordingHandle, Vec<ActionLogEntry>), StopRecordingControllerError>
    {
        match self.active.take() {
            Some(active) if active.id == recording_id => Ok((active.handle, active.actions)),
            other => {
                self.active = other;
                Err(StopRecordingControllerError::NoActiveRecording {
                    recording_id: recording_id.to_string(),
                })
            }
        }
    }

    /// Removes and returns the active recording for `recording_id` if it matches,
    /// used by the ffmpeg-exit watcher (scoped to a specific recording). Returns
    /// `None` (no-op) if a different recording is active or none is.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn take_by_id(
        &mut self,
        recording_id: &str,
    ) -> Option<(
        AIConversationId,
        computer_use::RecordingHandle,
        Vec<ActionLogEntry>,
    )> {
        match self.active.take() {
            Some(active) if active.id == recording_id => {
                Some((active.conversation_id, active.handle, active.actions))
            }
            other => {
                self.active = other;
                None
            }
        }
    }

    /// Removes and returns the active recording owned by `conversation_id`, used
    /// by the cancellation funnel to finalize a recording that is not tracked as
    /// an in-flight async action. Leaves a recording owned by another
    /// conversation in place.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn take_for_conversation(
        &mut self,
        conversation_id: AIConversationId,
    ) -> Option<(String, computer_use::RecordingHandle, Vec<ActionLogEntry>)> {
        match self.active.take() {
            Some(active) if active.conversation_id == conversation_id => {
                Some((active.id, active.handle, active.actions))
            }
            other => {
                self.active = other;
                None
            }
        }
    }

    /// Removes and returns whatever recording is active, used by the driver
    /// run-tail drain to finalize a still-running recording as the run ends.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn take_active(
        &mut self,
    ) -> Option<(
        String,
        AIConversationId,
        computer_use::RecordingHandle,
        Vec<ActionLogEntry>,
    )> {
        self.active.take().map(|active| {
            (
                active.id,
                active.conversation_id,
                active.handle,
                active.actions,
            )
        })
    }

    /// True while a start reservation is outstanding but no recording has been
    /// registered yet. The cancellation funnel uses this to release a start that
    /// is racing with cancellation.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn is_starting(&self) -> bool {
        self.starting
    }

    /// The id of the active recording, if any.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn active_recording_id(&self) -> Option<String> {
        self.active.as_ref().map(|active| active.id.clone())
    }

    /// Polls the active recording (if it matches `recording_id`) for an early
    /// exit of the capture process. Cheap and non-blocking; the watcher calls
    /// this on an interval to detect a duration/size cap hit or a crash.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub fn poll_active_exit(&mut self, recording_id: &str) -> Option<RecordingExitKind> {
        match self.active.as_mut() {
            Some(active) if active.id == recording_id => active.handle.poll_exit(),
            _ => None,
        }
    }
}

impl Entity for RecordingController {
    type Event = ();
}

impl SingletonEntity for RecordingController {}
