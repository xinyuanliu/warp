//! Process-global registry of in-flight artifact uploads (recordings,
//! screenshots, files). Cloud-agent teardown is not drained by the server, so
//! the driver run tail awaits [`ArtifactUploadState::drain`] to let uploads
//! spawned during a run finish before the process is torn down.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use warpui::r#async::Timer;

#[derive(Default)]
struct Inner {
    in_flight: AtomicUsize,
}

/// A cheap, cloneable handle to the runtime's in-flight-upload count. Obtain the
/// runtime instance via [`ArtifactUploadState::global`]; tests construct their
/// own with [`Default`].
#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Clone, Default)]
pub struct ArtifactUploadState {
    inner: Arc<Inner>,
}

/// RAII guard that counts one in-flight upload for its lifetime. Acquire it
/// synchronously at spawn time (so a concurrent [`ArtifactUploadState::drain`]
/// observes the upload) and move it into the upload future; it decrements on
/// drop.
#[cfg_attr(target_family = "wasm", allow(dead_code))]
pub struct ArtifactUploadGuard {
    inner: Arc<Inner>,
}

#[cfg_attr(target_family = "wasm", allow(dead_code))]
impl ArtifactUploadState {
    /// The per-process runtime instance.
    pub fn global() -> Self {
        static GLOBAL: OnceLock<ArtifactUploadState> = OnceLock::new();
        GLOBAL.get_or_init(ArtifactUploadState::default).clone()
    }

    /// Registers one in-flight upload; the returned guard decrements on drop.
    pub fn begin(&self) -> ArtifactUploadGuard {
        self.inner.in_flight.fetch_add(1, Ordering::SeqCst);
        ArtifactUploadGuard {
            inner: self.inner.clone(),
        }
    }

    /// The number of uploads currently in flight.
    pub fn in_flight(&self) -> usize {
        self.inner.in_flight.load(Ordering::SeqCst)
    }

    /// Waits until no uploads are in flight or `timeout` elapses. Returns whether
    /// the registry drained (`true`) or the timeout was hit (`false`).
    pub async fn drain(&self, timeout: Duration) -> bool {
        const POLL: Duration = Duration::from_millis(50);
        let mut waited = Duration::ZERO;
        while self.in_flight() > 0 {
            if waited >= timeout {
                return false;
            }
            Timer::after(POLL).await;
            waited += POLL;
        }
        true
    }
}

impl Drop for ArtifactUploadGuard {
    fn drop(&mut self) {
        self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "artifact_upload_state_tests.rs"]
mod tests;
