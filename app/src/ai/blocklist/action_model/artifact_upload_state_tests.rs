use std::time::Duration;

use super::*;

#[test]
fn begin_and_drop_adjust_in_flight() {
    let state = ArtifactUploadState::default();
    assert_eq!(state.in_flight(), 0);
    let first = state.begin();
    let second = state.begin();
    assert_eq!(state.in_flight(), 2);
    drop(first);
    assert_eq!(state.in_flight(), 1);
    drop(second);
    assert_eq!(state.in_flight(), 0);
}

#[tokio::test]
async fn drain_returns_true_when_empty() {
    let state = ArtifactUploadState::default();
    assert!(state.drain(Duration::from_secs(1)).await);
}

#[tokio::test]
async fn drain_times_out_while_upload_in_flight() {
    let state = ArtifactUploadState::default();
    let _guard = state.begin();
    // The guard is held, so drain can never observe zero and must time out.
    assert!(!state.drain(Duration::from_millis(150)).await);
}
