use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use computer_use::testing::MockRecorder;
use computer_use::{
    ActionLogEntry, RecordingCompletionStatus, RecordingHandle, RecordingOutput,
    DEFAULT_PILL_DURATION,
};

use super::{finalize_recording, RecordingTerminalOutcome, RecordingUploader};
use crate::ai::agent_sdk::artifact_upload::{
    CompletedFileArtifactUpload, FileArtifactUploadRequest,
};
use crate::ai::blocklist::action_model::artifact_upload_state::ArtifactUploadState;
use crate::ai::blocklist::action_model::recording_controller::FinalizeReason;
use crate::server::server_api::ai::FileArtifactRecord;

/// A [`RecordingUploader`] that records call counts and returns a fixed result.
struct MockUploader {
    calls: Arc<AtomicUsize>,
    fail: bool,
}

impl MockUploader {
    fn ok() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            fail: true,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl RecordingUploader for MockUploader {
    async fn upload(
        &self,
        _request: FileArtifactUploadRequest,
    ) -> anyhow::Result<CompletedFileArtifactUpload> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            anyhow::bail!("mock upload failed");
        }
        Ok(CompletedFileArtifactUpload {
            artifact: FileArtifactRecord {
                artifact_uid: "artifact-123".to_string(),
                filepath: "recording.mp4".to_string(),
                description: None,
                mime_type: "video/mp4".to_string(),
                size_bytes: Some(1024),
            },
            size_bytes: 1024,
        })
    }
}

/// Writes a temp `.mp4` + sibling `.log` and returns a matching output plus both
/// paths, so a test can assert finalize removes them.
fn make_temp_output(completion: RecordingCompletionStatus) -> (RecordingOutput, PathBuf, PathBuf) {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!("warp-rec-test-{}-{unique}", std::process::id()));
    let mp4 = base.with_extension("mp4");
    let log = base.with_extension("log");
    std::fs::write(&mp4, b"video-bytes").unwrap();
    std::fs::write(&log, b"log-bytes").unwrap();
    let output = RecordingOutput {
        path: mp4.clone(),
        duration: Duration::from_secs(2),
        width: 100,
        height: 200,
        size_bytes: 11,
        completion_status: completion,
    };
    (output, mp4, log)
}

#[tokio::test]
async fn published_on_successful_stop_and_upload() {
    let state = ArtifactUploadState::default();
    let guard = state.begin();
    let (output, mp4, log) = make_temp_output(RecordingCompletionStatus::Completed);
    let recorder = Box::new(MockRecorder::with_exact_output(output));
    let uploader = Arc::new(MockUploader::ok());
    let handle = RecordingHandle::new_test(100, 200).0;

    let outcome = finalize_recording(
        recorder,
        uploader.clone(),
        guard,
        handle,
        Vec::new(),
        FinalizeReason::StoppedByAgent,
        None,
    )
    .await;

    match outcome {
        RecordingTerminalOutcome::Published {
            artifact_uid,
            termination_reason,
            ..
        } => {
            assert_eq!(artifact_uid, "artifact-123");
            assert_eq!(termination_reason, "Stopped by agent");
        }
        other => panic!("expected Published, got {other:?}"),
    }
    assert_eq!(uploader.calls(), 1, "exactly one upload");
    assert!(!mp4.exists(), "mp4 temp should be removed");
    assert!(!log.exists(), "log temp should be removed");
    assert_eq!(
        state.in_flight(),
        0,
        "upload guard dropped when finalize ends"
    );
}

#[tokio::test]
async fn failed_when_upload_errors_and_temp_cleaned() {
    let state = ArtifactUploadState::default();
    let guard = state.begin();
    let (output, mp4, log) = make_temp_output(RecordingCompletionStatus::Completed);
    let recorder = Box::new(MockRecorder::with_exact_output(output));
    let uploader = Arc::new(MockUploader::failing());
    let handle = RecordingHandle::new_test(100, 200).0;

    let outcome = finalize_recording(
        recorder,
        uploader.clone(),
        guard,
        handle,
        Vec::new(),
        FinalizeReason::StoppedByAgent,
        None,
    )
    .await;

    assert!(matches!(outcome, RecordingTerminalOutcome::Failed { .. }));
    assert_eq!(uploader.calls(), 1);
    assert!(!mp4.exists(), "mp4 temp removed even on upload failure");
    assert!(!log.exists(), "log temp removed even on upload failure");
}

#[tokio::test]
async fn discarded_when_stop_fails_on_cancel() {
    let state = ArtifactUploadState::default();
    let guard = state.begin();
    let recorder = Box::new(MockRecorder::with_stop_error("no capture"));
    let uploader = Arc::new(MockUploader::ok());
    let handle = RecordingHandle::new_test(1, 1).0;

    let outcome = finalize_recording(
        recorder,
        uploader.clone(),
        guard,
        handle,
        Vec::new(),
        FinalizeReason::Cancelled,
        None,
    )
    .await;

    assert!(matches!(
        outcome,
        RecordingTerminalOutcome::Discarded { .. }
    ));
    assert_eq!(uploader.calls(), 0, "no upload attempted when stop failed");
}

#[tokio::test]
async fn failed_when_stop_fails_on_explicit_stop() {
    let state = ArtifactUploadState::default();
    let guard = state.begin();
    let recorder = Box::new(MockRecorder::with_stop_error("boom"));
    let uploader = Arc::new(MockUploader::ok());
    let handle = RecordingHandle::new_test(1, 1).0;

    let outcome = finalize_recording(
        recorder,
        uploader,
        guard,
        handle,
        Vec::new(),
        FinalizeReason::StoppedByAgent,
        None,
    )
    .await;

    assert!(matches!(outcome, RecordingTerminalOutcome::Failed { .. }));
}

#[tokio::test]
async fn limit_reached_maps_termination_reason() {
    let state = ArtifactUploadState::default();
    let guard = state.begin();
    let (output, ..) = make_temp_output(RecordingCompletionStatus::StoppedEarly);
    let recorder = Box::new(MockRecorder::with_exact_output(output));
    let uploader = Arc::new(MockUploader::ok());
    let handle = RecordingHandle::new_test(1, 1).0;

    let outcome = finalize_recording(
        recorder,
        uploader,
        guard,
        handle,
        Vec::new(),
        FinalizeReason::LimitReached,
        None,
    )
    .await;

    match outcome {
        RecordingTerminalOutcome::Published {
            termination_reason,
            completion_status,
            ..
        } => {
            assert_eq!(
                termination_reason,
                "Stopped at the configured duration or size limit"
            );
            assert_eq!(completion_status, RecordingCompletionStatus::StoppedEarly);
        }
        other => panic!("expected Published, got {other:?}"),
    }
}

#[tokio::test]
async fn published_with_overlay_entries_uploads_once() {
    // Off-Linux the burn-in is a no-op that returns the original file, so this
    // exercises the burn-in hook path (non-empty action log) without a real
    // ffmpeg: the recording is still published with exactly one upload.
    let state = ArtifactUploadState::default();
    let guard = state.begin();
    let (output, mp4, log) = make_temp_output(RecordingCompletionStatus::Completed);
    let recorder = Box::new(MockRecorder::with_exact_output(output));
    let uploader = Arc::new(MockUploader::ok());
    let handle = RecordingHandle::new_test(1280, 720).0;
    let entries = vec![
        ActionLogEntry {
            offset: Duration::from_millis(500),
            labels: vec!["ctrl+a".to_string()],
            show_duration: DEFAULT_PILL_DURATION,
        },
        ActionLogEntry {
            offset: Duration::from_millis(2000),
            labels: vec!["typing\u{2026}".to_string(), "scroll \u{2193}".to_string()],
            show_duration: DEFAULT_PILL_DURATION,
        },
    ];

    let outcome = finalize_recording(
        recorder,
        uploader.clone(),
        guard,
        handle,
        entries,
        FinalizeReason::StoppedByAgent,
        None,
    )
    .await;

    assert!(matches!(
        outcome,
        RecordingTerminalOutcome::Published { .. }
    ));
    assert_eq!(
        uploader.calls(),
        1,
        "exactly one upload with overlay entries"
    );
    assert!(!mp4.exists(), "mp4 temp removed");
    assert!(!log.exists(), "log temp removed");
}
