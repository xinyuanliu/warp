//! Test-only, in-memory [`Recorder`] used to exercise recording finalization
//! without a real ffmpeg capture. Off-Linux only (the real [`RecordingHandle`]
//! capture fields are absent there, so a handle can be constructed by tests).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::{
    Recorder, RecordingCompletionStatus, RecordingConfig, RecordingError, RecordingExitState,
    RecordingHandle, RecordingOutput,
};

struct MockRecorderState {
    width: u32,
    height: u32,
    /// What `stop` returns: the finalized output on success, or a reason string
    /// that becomes a [`RecordingError::Finalize`].
    stop_result: Result<RecordingOutput, String>,
    started: usize,
    stopped: usize,
    /// Exit flag of the most recently started handle, so a test can simulate an
    /// early exit (limit/crash) that a watcher observes via `poll_exit`.
    last_exit_state: Option<RecordingExitState>,
}

/// A cloneable, configurable recorder for tests. `start` yields a handle whose
/// shared exit flag is exposed via [`MockRecorder::last_exit_state`]; `stop`
/// returns the preconfigured output or error.
#[derive(Clone)]
pub struct MockRecorder {
    inner: Arc<Mutex<MockRecorderState>>,
}

/// Builds a default successful output pointing at `path`.
pub fn default_output(path: PathBuf) -> RecordingOutput {
    RecordingOutput {
        path,
        duration: Duration::from_secs(1),
        width: 1280,
        height: 720,
        size_bytes: 1024,
        completion_status: RecordingCompletionStatus::Completed,
    }
}

impl MockRecorder {
    fn from_state(stop_result: Result<RecordingOutput, String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(MockRecorderState {
                width: 1280,
                height: 720,
                stop_result,
                started: 0,
                stopped: 0,
                last_exit_state: None,
            })),
        }
    }

    /// A recorder whose `stop` yields a successful output at `path`.
    pub fn with_output(path: PathBuf) -> Self {
        Self::from_state(Ok(default_output(path)))
    }

    /// A recorder whose `stop` yields the given output verbatim.
    pub fn with_exact_output(output: RecordingOutput) -> Self {
        Self::from_state(Ok(output))
    }

    /// A recorder whose `stop` fails with `reason`.
    pub fn with_stop_error(reason: impl Into<String>) -> Self {
        Self::from_state(Err(reason.into()))
    }

    /// Number of times `start` has been called.
    pub fn started_count(&self) -> usize {
        self.inner.lock().expect("mock recorder poisoned").started
    }

    /// Number of times `stop` has been called.
    pub fn stopped_count(&self) -> usize {
        self.inner.lock().expect("mock recorder poisoned").stopped
    }

    /// Shared exit flag of the last started handle, or `None` before `start`.
    pub fn last_exit_state(&self) -> Option<RecordingExitState> {
        self.inner
            .lock()
            .expect("mock recorder poisoned")
            .last_exit_state
            .clone()
    }
}

#[async_trait]
impl Recorder for MockRecorder {
    async fn start(&self, _config: RecordingConfig) -> Result<RecordingHandle, RecordingError> {
        let mut state = self.inner.lock().expect("mock recorder poisoned");
        state.started += 1;
        let (handle, exit_state) = RecordingHandle::new_test(state.width, state.height);
        state.last_exit_state = Some(exit_state);
        Ok(handle)
    }

    async fn stop(&self, _handle: RecordingHandle) -> Result<RecordingOutput, RecordingError> {
        let mut state = self.inner.lock().expect("mock recorder poisoned");
        state.stopped += 1;
        match &state.stop_result {
            Ok(output) => Ok(output.clone()),
            Err(reason) => Err(RecordingError::Finalize {
                reason: reason.clone(),
            }),
        }
    }
}
