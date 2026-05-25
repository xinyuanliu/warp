//! Shared protocol, discovery, authentication, and client types for local Warp control.
//!
//! The `local_control` crate is intentionally UI-agnostic so the Warp app and
//! `warpctrl` CLI can share the same wire envelopes, action catalog, discovery
//! records, selectors, and credential validation rules.
pub mod auth;
pub mod catalog;
pub mod client;
pub mod discovery;
pub mod protocol;
pub mod selection;
pub mod selectors;

pub use auth::{
    AuthToken, AuthenticatedUserGrant, CredentialGrant, CredentialRequest, ScopedCredential,
};
pub use catalog::{
    ActionImplementationStatus, ActionKind, ActionMetadata, AuthenticatedUserRequirement,
    InvocationContext, PermissionCategory, RiskTier, StateDataCategory, TargetScope,
};
pub use discovery::{
    ControlEndpoint, CredentialBrokerReference, InstanceId, InstanceRecord, RegisteredInstance,
    discovery_dir,
};
pub use protocol::{
    Action, ActionGetParams, ActionGetResult, ActionListParams, ActionListResult,
    ActiveTargetChain, AppActiveParams, AppInspectParams, AppInspectResult, AppVersionResult,
    AppearanceStateResult, BlockGetParams, BlockGetResult, BlockListParams, BlockListResult,
    BlockSummary, ControlError, ControlResponse, DriveCreateParams, DriveDeleteParams,
    DriveInsertParams, DriveMutationResult, DriveObjectSummary, DriveObjectType, DriveRunParams,
    DriveTarget, DriveUpdateParams, EmptyParams, ErrorCode, ErrorResponseEnvelope,
    ExecutionContextProof, FileDeleteParams, FileMutationResult, FileTarget, FileWriteParams,
    HistoryEntrySummary, HistoryListParams, HistoryListResult, InputGetParams, InputStateResult,
    PROTOCOL_VERSION, PaneListResult, PaneSummary, RequestEnvelope, ResponseEnvelope,
    SessionListResult, SessionSummary, SettingGetParams, SettingGetResult, SettingListParams,
    SettingListResult, SettingSummary, TabListResult, TabSummary, ThemeListResult, ThemeSummary,
    WindowListResult, WindowSummary,
};
pub use selectors::{
    DriveObjectSelector, FileSelector, PaneSelector, SessionSelector, TabSelector, TargetSelector,
    WindowSelector,
};
