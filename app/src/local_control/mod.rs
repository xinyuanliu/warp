use crate::auth::AuthStateProvider;
use crate::cloud_object::{
    model::persistence::CloudModel, CloudObject, GenericStringObjectFormat, JsonObjectType,
    ObjectType,
};
use crate::code::view::CodeView;
use crate::env_vars::CloudEnvVarCollection;
use crate::features::FeatureFlag;
use crate::notebooks::CloudNotebook;
use crate::pane_group::{PaneGroup, PaneId};
use crate::projects::ProjectManagementModel;
use crate::terminal::model::session::SessionId;
use crate::terminal::model::TerminalModel;
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use crate::settings::{
    AccessibilitySettings, FontSettings, InputSettings, LocalControlInvocationContext,
    LocalControlPermissionCategory, LocalControlSettings, ThemeSettings,
};
use crate::terminal::view::TerminalView;
use crate::terminal::History;
use crate::themes::theme::ThemeKind;
use crate::user_config::WarpConfig;
use crate::workflows::CloudWorkflow;
use crate::workspace::ActiveSession;
use crate::WindowSettings;
use ::local_control::auth::{CredentialGrant, CredentialRequest, ScopedCredential};
use ::local_control::protocol::{
    ActionGetParams, AppearanceStateResult, BlockGetParams, BlockGetResult, BlockListParams,
    BlockListResult, BlockSummary, DriveGetParams, DriveGetResult, DriveListParams,
    DriveListResult, DriveObjectSummary, DriveObjectType as ControlDriveObjectType, DriveTarget,
    FileListResult, FileSummary, HistoryEntrySummary, HistoryListParams, HistoryListResult,
    InputStateResult, PaneTarget, ProjectActiveResult, ProjectListResult, ProjectSummary,
    SessionTarget, SettingGetParams, SettingGetResult, SettingListResult, SettingSummary,
    TabTarget, TargetSelector, ThemeListResult, ThemeSummary, WindowTarget,
};
use ::local_control::{
    ActionKind, AuthToken, ControlEndpoint, ControlError, ControlResponse, ErrorCode,
    ErrorResponseEnvelope, InstanceId, InstanceRecord, RegisteredInstance, RequestEnvelope,
    ResponseEnvelope, PROTOCOL_VERSION,
};
use ::local_control::{InvocationContext, PermissionCategory};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use chrono::Duration;
use serde_json::json;
use serde_json::Value;
use settings::Setting as _;
use warp_core::channel::ChannelState;
use warpui::{Entity, ModelContext, ModelSpawner, SingletonEntity, TypedActionView, ViewHandle};

use crate::workspace::{Workspace, WorkspaceAction};

struct ResolvedTerminalTarget {
    terminal_view: ViewHandle<TerminalView>,
}

#[derive(Clone)]
struct ControlServerState {
    bridge_spawner: ModelSpawner<LocalControlBridge>,
    instance_id: InstanceId,
    credentials: Arc<Mutex<HashMap<String, CredentialGrant>>>,
}

pub struct LocalControlServer {
    _runtime: Option<tokio::runtime::Runtime>,
    _registered_instance: Option<RegisteredInstance>,
}

impl Entity for LocalControlServer {
    type Event = ();
}

impl SingletonEntity for LocalControlServer {}

impl LocalControlServer {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        if !warp_control_cli_enabled() {
            return Self {
                _runtime: None,
                _registered_instance: None,
            };
        }
        match Self::start(ctx) {
            Ok(server) => server,
            Err(error) => {
                log::warn!("Failed to start local-control server: {error:#}");
                Self {
                    _runtime: None,
                    _registered_instance: None,
                }
            }
        }
    }

    fn start(ctx: &mut ModelContext<Self>) -> Result<Self, ControlError> {
        ensure_feature_enabled()?;
        if !outside_warp_any_implemented_action_enabled(ctx) {
            return Err(ControlError::new(
                ErrorCode::LocalControlDisabled,
                "outside-Warp local control is disabled",
            ));
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_io()
            .build()
            .map_err(|err| {
                ControlError::with_details(
                    ErrorCode::Internal,
                    "failed to create local-control runtime",
                    err.to_string(),
                )
            })?;
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind(SocketAddr::from((
                [127, 0, 0, 1],
                0,
            ))))
            .map_err(|err| {
                ControlError::with_details(
                    ErrorCode::Internal,
                    "failed to bind local-control listener",
                    err.to_string(),
                )
            })?;
        let port = listener.local_addr().map_err(|err| {
            ControlError::with_details(
                ErrorCode::Internal,
                "failed to read local-control listener address",
                err.to_string(),
            )
        })?;
        let outside_warp_control_enabled = LocalControlSettings::as_ref(ctx)
            .is_context_enabled(LocalControlInvocationContext::OutsideWarp);
        let endpoint =
            outside_warp_control_enabled.then_some(ControlEndpoint::localhost(port.port()));
        let record = InstanceRecord::for_current_process(
            endpoint,
            ChannelState::channel().to_string(),
            ChannelState::app_id().to_string(),
            ChannelState::app_version().map(str::to_owned),
            ActionKind::implemented_metadata(),
        );
        let instance_id = record.instance_id.clone();
        let bridge_spawner = LocalControlBridge::handle(ctx).update(ctx, |bridge, ctx| {
            bridge.set_instance_id(instance_id.clone());
            ctx.spawner()
        });
        let registered_instance = RegisteredInstance::register(record)?;
        let state = ControlServerState {
            bridge_spawner,
            instance_id,
            credentials: Arc::default(),
        };
        let router = Router::new()
            .route("/v1/control", post(handle_control_request))
            .route("/v1/control/credentials", post(handle_credential_request))
            .with_state(state);
        runtime.spawn(async move {
            if let Err(err) = axum::serve(listener, router).await {
                log::warn!("local-control listener stopped: {err:#}");
            }
        });
        Ok(Self {
            _runtime: Some(runtime),
            _registered_instance: Some(registered_instance),
        })
    }
}

pub struct LocalControlBridge {
    instance_id: Option<InstanceId>,
}

impl Entity for LocalControlBridge {
    type Event = ();
}

impl SingletonEntity for LocalControlBridge {}

impl LocalControlBridge {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self { instance_id: None }
    }

    fn set_instance_id(&mut self, instance_id: InstanceId) {
        self.instance_id = Some(instance_id);
    }

    fn handle_request(
        &mut self,
        request: RequestEnvelope,
        grant: CredentialGrant,
        ctx: &mut ModelContext<Self>,
    ) -> ResponseEnvelope {
        if let Err(error) = ensure_feature_enabled() {
            return ResponseEnvelope::error(request.request_id, error);
        }
        if request.protocol_version != PROTOCOL_VERSION {
            return ResponseEnvelope::error(
                request.request_id,
                ControlError::new(
                    ErrorCode::ProtocolVersionUnsupported,
                    format!("unsupported protocol version {}", request.protocol_version),
                ),
            );
        }
        if let Err(error) = validate_action_params(&request.action) {
            return ResponseEnvelope::error(request.request_id, error);
        }
        if let Err(error) = grant.verify_for_action(request.action.kind) {
            return ResponseEnvelope::error(request.request_id, error);
        }
        if !request.action.kind.is_implemented() {
            return ResponseEnvelope::error(
                request.request_id,
                ControlError::new(
                    ErrorCode::UnsupportedAction,
                    format!(
                        "{} is not implemented by this local-control bridge",
                        request.action.kind.as_str()
                    ),
                ),
            );
        }
        match request.action.kind {
            ActionKind::InstanceList => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                ResponseEnvelope::ok(request.request_id, self.instance_metadata())
            }
            ActionKind::AppPing => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                ResponseEnvelope::ok(request.request_id, self.ping_metadata())
            }
            ActionKind::AppVersion => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                ResponseEnvelope::ok(request.request_id, self.version_metadata())
            }
            ActionKind::AppInspect => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                ResponseEnvelope::ok(request.request_id, self.inspect_metadata())
            }
            ActionKind::ActionList => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                ResponseEnvelope::ok(request.request_id, self.action_list_metadata())
            }
            ActionKind::ActionGet => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.action_get_metadata(&request.action) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::ThemeList => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match theme_list_result(ctx).and_then(to_control_data) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::AppearanceGet => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match appearance_state_result(ctx).and_then(to_control_data) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::SettingGet => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match request
                    .action
                    .params_as::<SettingGetParams>()
                    .and_then(|params| setting_get_result(&params.key, ctx))
                    .and_then(to_control_data)
                {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::SettingList => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match setting_list_result(ctx).and_then(to_control_data) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::FileList => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.list_open_files(&request.target, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::ProjectActive => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.active_project(&request.target, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::ProjectList => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.list_projects(&request.target, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::InputGet => {
                if let Err(error) = ensure_authenticated_user_matches(&grant, ctx) {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.get_input_state(&request.target, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::HistoryList => {
                if let Err(error) = ensure_authenticated_user_matches(&grant, ctx) {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match request
                    .action
                    .params_as::<HistoryListParams>()
                    .and_then(|params| self.list_history(&request.target, params, ctx))
                {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::TabCreate => {
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.create_terminal_tab(&request.target, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::BlockList => {
                if let Err(error) = ensure_authenticated_user_matches(&grant, ctx) {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.list_blocks(&request.target, request.action.params_as(), ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::BlockGet => {
                if let Err(error) = ensure_authenticated_user_matches(&grant, ctx) {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.get_block(&request.target, request.action.params_as(), ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::DriveList => {
                if let Err(error) = ensure_authenticated_user_matches(&grant, ctx) {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.list_drive_objects(&request, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            ActionKind::DriveGet => {
                if let Err(error) = ensure_authenticated_user_matches(&grant, ctx) {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                if let Err(error) =
                    ensure_action_allowed(grant.invocation_context, request.action.kind, ctx)
                {
                    return ResponseEnvelope::error(request.request_id, error);
                }
                match self.get_drive_object(&request, ctx) {
                    Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                    Err(error) => ResponseEnvelope::error(request.request_id, error),
                }
            }
            action => ResponseEnvelope::error(
                request.request_id,
                ControlError::new(
                    ErrorCode::UnsupportedAction,
                    format!(
                        "{} is not implemented by this local-control bridge",
                        action.as_str()
                    ),
                ),
            ),
        }
    }

    fn create_terminal_tab(
        &mut self,
        target: &TargetSelector,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_tab_create_target(target)?;
        let window_id = target_window_id(ctx)?;
        let workspace = ctx
            .views_of_type::<Workspace>(window_id)
            .and_then(|workspaces| workspaces.into_iter().next())
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::MissingTarget,
                    "tab.create requires a workspace in the target window",
                )
            })?;
        let (previous_tab_count, tab_count, active_tab_index) =
            workspace.update(ctx, |workspace, ctx| {
                let previous_tab_count = workspace.tab_count();
                workspace.handle_action(
                    &WorkspaceAction::AddTerminalTab {
                        hide_homepage: false,
                    },
                    ctx,
                );
                (
                    previous_tab_count,
                    workspace.tab_count(),
                    workspace.active_tab_index(),
                )
            });
        Ok(json!({
            "action": ActionKind::TabCreate.as_str(),
            "created": true,
            "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
            "window": {
                "selector": "active",
                "id": window_id.to_string(),
            },
            "tab": {
                "previous_count": previous_tab_count,
                "count": tab_count,
                "active_index": active_tab_index,
            },
        }))
    }

    fn list_open_files(
        &mut self,
        target: &TargetSelector,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_instance_metadata_read_target(ActionKind::FileList, target)?;
        to_control_data(FileListResult {
            files: open_file_summaries(ctx),
        })
    }

    fn active_project(
        &mut self,
        target: &TargetSelector,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_instance_metadata_read_target(ActionKind::ProjectActive, target)?;
        to_control_data(ProjectActiveResult {
            project: active_project_summary(ctx),
        })
    }

    fn list_projects(
        &mut self,
        target: &TargetSelector,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_instance_metadata_read_target(ActionKind::ProjectList, target)?;
        to_control_data(ProjectListResult {
            projects: project_summaries(ctx),
        })
    }

    fn get_input_state(
        &mut self,
        target: &TargetSelector,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        let resolved = resolve_terminal_read_target(ActionKind::InputGet, target, ctx)?;
        let session_id = resolved
            .terminal_view
            .read(ctx, |terminal, _| terminal.active_block_session_id())
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::MissingTarget,
                    "input.get requires a target terminal session",
                )
            })?;
        let input = resolved
            .terminal_view
            .read(ctx, |terminal, _| terminal.input().clone());
        let (text, cursor_offset) = input.read(ctx, |input, ctx| {
            let cursor_offset = input
                .editor()
                .as_ref(ctx)
                .start_byte_index_of_first_selection(ctx)
                .as_usize();
            (input.buffer_text(ctx), cursor_offset)
        });
        let cursor_offset = u32::try_from(cursor_offset).map_err(|err| {
            ControlError::with_details(
                ErrorCode::Internal,
                "input cursor offset is too large to encode",
                err.to_string(),
            )
        })?;
        to_control_data(InputStateResult {
            session_id: session_id.as_u64().to_string(),
            text,
            cursor_offset,
        })
    }

    fn list_history(
        &mut self,
        target: &TargetSelector,
        params: HistoryListParams,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        let resolved = resolve_terminal_read_target(ActionKind::HistoryList, target, ctx)?;
        let session_id = resolved
            .terminal_view
            .read(ctx, |terminal, _| terminal.active_block_session_id())
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::MissingTarget,
                    "history.list requires a target terminal session",
                )
            })?;
        let commands = History::as_ref(ctx)
            .is_queryable(&session_id)
            .then(|| {
                History::as_ref(ctx)
                    .commands_shared(session_id)
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let start_index = params
            .limit
            .and_then(|limit| usize::try_from(limit).ok())
            .map(|limit| commands.len().saturating_sub(limit))
            .unwrap_or_default();
        let entries = commands
            .iter()
            .enumerate()
            .skip(start_index)
            .map(|(index, entry)| HistoryEntrySummary {
                entry_id: format!("history:{}:{index}", session_id.as_u64()),
                command: entry.command.clone(),
                cwd: entry.pwd.clone(),
            })
            .collect();
        to_control_data(HistoryListResult { entries })
    }

    fn list_drive_objects(
        &mut self,
        request: &RequestEnvelope,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_drive_target(&request.target, request.action.kind)?;
        let params = request.action.params_as::<DriveListParams>()?;
        let cloud_model = CloudModel::as_ref(ctx);
        let mut objects = cloud_model
            .cloud_objects()
            .filter_map(|object| drive_object_summary(object.as_ref()))
            .filter(|summary| {
                params
                    .object_type
                    .is_none_or(|object_type| summary.object_type == object_type)
            })
            .collect::<Vec<_>>();
        objects.sort_by(|a, b| {
            drive_object_type_rank(a.object_type)
                .cmp(&drive_object_type_rank(b.object_type))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        serde_json::to_value(DriveListResult { objects }).map_err(json_response_error)
    }

    fn get_drive_object(
        &mut self,
        request: &RequestEnvelope,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_drive_target(&request.target, request.action.kind)?;
        let params = request.action.params_as::<DriveGetParams>()?;
        if let Some(DriveTarget::Id { object_type, id }) = request.target.drive.as_ref() {
            if *object_type != params.object_type || id.0 != params.id {
                return Err(ControlError::new(
                    ErrorCode::TargetStateConflict,
                    "drive.get target selector does not match the requested Drive object",
                ));
            }
        }
        let cloud_model = CloudModel::as_ref(ctx);
        let object = cloud_model.get_by_uid(&params.id).ok_or_else(|| {
            ControlError::new(
                ErrorCode::StaleTarget,
                "drive.get could not resolve the requested Drive object id",
            )
        })?;
        drive_object_get_result(object, params.object_type)
            .and_then(|result| serde_json::to_value(result).map_err(json_response_error))
    }
    fn list_blocks(
        &self,
        target: &TargetSelector,
        params: Result<BlockListParams, ControlError>,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_block_list_target(target)?;
        let params = params?;
        let terminal = target_terminal_view(ctx)?;
        let result = terminal.read(ctx, |view, _| {
            let session_id = resolve_session_selector(
                target.session.as_ref(),
                view.active_block_session_id(),
                ActionKind::BlockList,
            )?;
            let model = view.model.lock();
            block_list_result_from_model(&model, session_id, target.session.is_some(), params)
        })?;
        serde_json::to_value(result).map_err(|err| {
            ControlError::with_details(
                ErrorCode::Internal,
                "failed to serialize block.list result",
                err.to_string(),
            )
        })
    }

    fn get_block(
        &self,
        target: &TargetSelector,
        params: Result<BlockGetParams, ControlError>,
        ctx: &mut ModelContext<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        validate_block_get_target(target)?;
        let params = params?;
        let terminal = target_terminal_view(ctx)?;
        let result = terminal.read(ctx, |view, _| {
            let session_id = resolve_session_selector(
                target.session.as_ref(),
                view.active_block_session_id(),
                ActionKind::BlockGet,
            )?;
            let model = view.model.lock();
            block_get_result_from_model(&model, session_id, &params.block_id)
        })?;
        serde_json::to_value(result).map_err(|err| {
            ControlError::with_details(
                ErrorCode::Internal,
                "failed to serialize block.get result",
                err.to_string(),
            )
        })
    }

    fn instance_metadata(&self) -> serde_json::Value {
        json!({
            "action": ActionKind::InstanceList.as_str(),
            "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
            "pid": std::process::id(),
            "channel": ChannelState::channel().to_string(),
            "app_id": ChannelState::app_id().to_string(),
            "app_version": ChannelState::app_version(),
            "protocol_version": PROTOCOL_VERSION,
            "actions": ActionKind::implemented_metadata(),
        })
    }

    fn ping_metadata(&self) -> serde_json::Value {
        json!({
            "action": ActionKind::AppPing.as_str(),
            "ok": true,
            "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
            "protocol_version": PROTOCOL_VERSION,
        })
    }

    fn version_metadata(&self) -> serde_json::Value {
        json!({
            "action": ActionKind::AppVersion.as_str(),
            "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
            "protocol_version": PROTOCOL_VERSION,
            "channel": ChannelState::channel().to_string(),
            "app_id": ChannelState::app_id().to_string(),
            "app_version": ChannelState::app_version(),
        })
    }

    fn inspect_metadata(&self) -> serde_json::Value {
        json!({
            "action": ActionKind::AppInspect.as_str(),
            "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
            "version": {
                "protocol_version": PROTOCOL_VERSION,
                "channel": ChannelState::channel().to_string(),
                "app_id": ChannelState::app_id().to_string(),
                "app_version": ChannelState::app_version(),
            },
            "active": {
                "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
            },
            "actions": ActionKind::implemented_metadata(),
        })
    }

    fn action_list_metadata(&self) -> serde_json::Value {
        json!({
            "action": ActionKind::ActionList.as_str(),
            "actions": ActionKind::implemented_metadata(),
        })
    }

    fn action_get_metadata(
        &self,
        action: &::local_control::Action,
    ) -> Result<serde_json::Value, ControlError> {
        let params = action.params_as::<ActionGetParams>()?;
        let metadata = action_metadata_for_name(&params.action)?;
        Ok(json!({
            "action": ActionKind::ActionGet.as_str(),
            "requested_action": params.action,
            "metadata": metadata,
        }))
    }
}

const ALLOWLISTED_SETTING_KEYS: &[&str] = &[
    "accessibility.accessibility_verbosity",
    "appearance.text.font_name",
    "appearance.text.font_size",
    "appearance.themes.dark_theme",
    "appearance.themes.light_theme",
    "appearance.themes.system_theme",
    "appearance.themes.theme",
    "appearance.window.zoom_level",
    "terminal.input.error_underlining_enabled",
    "terminal.input.syntax_highlighting",
];

const PRIVATE_OR_SENSITIVE_SETTING_KEYS: &[&str] = &[
    "local_control.allow_inside_warp_control",
    "local_control.allow_inside_warp_read_only",
    "local_control.allow_inside_warp_read_write",
    "local_control.allow_outside_warp_control",
    "local_control.allow_outside_warp_read_only",
    "local_control.allow_outside_warp_read_write",
    "terminal.input.autosuggestion_accepted_count",
    "terminal.input.inline_menu_custom_content_heights",
    "terminal.input.workflows_box_expanded",
];

fn theme_list_result(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<ThemeListResult, ControlError> {
    let current_theme = active_theme_kind(ThemeSettings::as_ref(ctx), ctx);
    let mut themes = WarpConfig::as_ref(ctx)
        .theme_config()
        .theme_items()
        .map(|(kind, _)| ThemeSummary {
            name: public_theme_name(kind),
            is_current: *kind == current_theme,
        })
        .collect::<Vec<_>>();
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ThemeListResult { themes })
}

fn appearance_state_result(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<AppearanceStateResult, ControlError> {
    let theme_settings = ThemeSettings::as_ref(ctx);
    let font_settings = FontSettings::as_ref(ctx);
    let window_settings = WindowSettings::as_ref(ctx);
    let system_themes = theme_settings.selected_system_themes.value();
    Ok(AppearanceStateResult {
        theme: Some(public_theme_name(theme_settings.theme_kind.value())),
        follow_system_theme: *theme_settings.use_system_theme.value(),
        light_theme: Some(public_theme_name(&system_themes.light)),
        dark_theme: Some(public_theme_name(&system_themes.dark)),
        font_size: rounded_u32(*font_settings.monospace_font_size.value()),
        ui_zoom_percent: Some(u32::from(*window_settings.zoom_level.value())),
    })
}

fn setting_list_result(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<SettingListResult, ControlError> {
    let settings = ALLOWLISTED_SETTING_KEYS
        .iter()
        .map(|key| setting_summary_for_key(key, ctx))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SettingListResult { settings })
}

fn setting_get_result(
    key: &str,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<SettingGetResult, ControlError> {
    Ok(SettingGetResult {
        setting: setting_summary_for_key(key, ctx)?,
    })
}

fn setting_summary_for_key(
    key: &str,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<SettingSummary, ControlError> {
    let theme_settings = ThemeSettings::as_ref(ctx);
    let font_settings = FontSettings::as_ref(ctx);
    let input_settings = InputSettings::as_ref(ctx);
    let accessibility_settings = AccessibilitySettings::as_ref(ctx);
    let window_settings = WindowSettings::as_ref(ctx);
    match key {
        "appearance.themes.theme" => Ok(setting_summary(
            key,
            json!(public_theme_name(theme_settings.theme_kind.value())),
            "string",
        )),
        "appearance.themes.system_theme" => Ok(setting_summary(
            key,
            json!(*theme_settings.use_system_theme.value()),
            "bool",
        )),
        "appearance.themes.light_theme" => Ok(setting_summary(
            key,
            json!(public_theme_name(
                &theme_settings.selected_system_themes.value().light
            )),
            "string",
        )),
        "appearance.themes.dark_theme" => Ok(setting_summary(
            key,
            json!(public_theme_name(
                &theme_settings.selected_system_themes.value().dark
            )),
            "string",
        )),
        "appearance.text.font_name" => Ok(setting_summary(
            key,
            json!(font_settings.monospace_font_name.value()),
            "string",
        )),
        "appearance.text.font_size" => Ok(setting_summary(
            key,
            json!(*font_settings.monospace_font_size.value()),
            "number",
        )),
        "appearance.window.zoom_level" => Ok(setting_summary(
            key,
            json!(*window_settings.zoom_level.value()),
            "number",
        )),
        "terminal.input.syntax_highlighting" => Ok(setting_summary(
            key,
            json!(*input_settings.syntax_highlighting.value()),
            "bool",
        )),
        "terminal.input.error_underlining_enabled" => Ok(setting_summary(
            key,
            json!(*input_settings.error_underlining.value()),
            "bool",
        )),
        "accessibility.accessibility_verbosity" => Ok(setting_summary(
            key,
            json!(format!(
                "{:?}",
                accessibility_settings.a11y_verbosity.value()
            )),
            "string",
        )),
        _ => Err(rejected_setting_key(key)),
    }
}

fn setting_summary(key: &str, value: Value, value_type: &str) -> SettingSummary {
    SettingSummary {
        key: key.to_owned(),
        value,
        value_type: value_type.to_owned(),
    }
}

fn rejected_setting_key(key: &str) -> ControlError {
    if PRIVATE_OR_SENSITIVE_SETTING_KEYS.contains(&key) {
        return ControlError::new(
            ErrorCode::NotAllowlisted,
            format!("{key} is private or sensitive and is not available through local control"),
        );
    }
    ControlError::new(
        ErrorCode::NotAllowlisted,
        format!("{key} is not an allowlisted local-control setting"),
    )
}

fn public_theme_name(theme: &ThemeKind) -> String {
    match theme {
        ThemeKind::Custom(custom) | ThemeKind::CustomBase16(custom) => custom.name(),
        ThemeKind::InMemory(_) => "In-memory theme".to_owned(),
        _ => theme.to_string(),
    }
}

fn active_theme_kind(
    theme_settings: &ThemeSettings,
    ctx: &ModelContext<LocalControlBridge>,
) -> ThemeKind {
    crate::settings::derived_theme_kind(theme_settings, ctx.system_theme())
}

fn rounded_u32(value: f32) -> Option<u32> {
    if value.is_finite() && value >= 0.0 && value <= u32::MAX as f32 {
        return Some(value.round() as u32);
    }
    None
}

async fn handle_credential_request(
    State(state): State<ControlServerState>,
    payload: Result<Json<CredentialRequest>, JsonRejection>,
) -> Response {
    if let Err(error) = ensure_feature_enabled() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponseEnvelope::new(error)),
        )
            .into_response();
    }
    let request = match payload {
        Ok(Json(request)) => request,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponseEnvelope::new(ControlError::with_details(
                    ErrorCode::InvalidRequest,
                    "failed to decode local-control credential request",
                    err.to_string(),
                ))),
            )
                .into_response();
        }
    };
    if request.protocol_version != PROTOCOL_VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseEnvelope::new(ControlError::new(
                ErrorCode::ProtocolVersionUnsupported,
                format!("unsupported protocol version {}", request.protocol_version),
            ))),
        )
            .into_response();
    }
    let metadata = request.action.metadata();
    if !request.action.is_implemented() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseEnvelope::new(ControlError::new(
                ErrorCode::UnsupportedAction,
                format!(
                    "{} is not implemented by this local-control bridge",
                    request.action.as_str()
                ),
            ))),
        )
            .into_response();
    }
    if !metadata
        .allowed_invocation_contexts
        .contains(&request.invocation_context)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponseEnvelope::new(ControlError::new(
                ErrorCode::ExecutionContextNotAllowed,
                format!(
                    "{} cannot run from the requested invocation context",
                    request.action.as_str()
                ),
            ))),
        )
            .into_response();
    }
    if let Err(error) = request.verify_execution_context_proof() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponseEnvelope::new(error)),
        )
            .into_response();
    }
    let authorization_check = state
        .bridge_spawner
        .spawn({
            let action = request.action;
            let invocation_context = request.invocation_context;
            move |_, ctx| {
                ensure_action_allowed(invocation_context, action, ctx)?;
                authenticated_user_subject_for_action(action, ctx)
            }
        })
        .await;
    let authenticated_subject = match authorization_check {
        Ok(Ok(subject)) => subject,
        Ok(Err(error)) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorResponseEnvelope::new(error)),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponseEnvelope::new(ControlError::new(
                    ErrorCode::BridgeUnavailable,
                    "local-control app bridge is unavailable",
                ))),
            )
                .into_response();
        }
    };
    let auth_token = AuthToken::generate();
    let mut grant = CredentialGrant::new(
        state.instance_id.clone(),
        request.action,
        request.invocation_context,
        Duration::minutes(5),
    );
    grant.authenticated_user.subject = authenticated_subject;
    let mut credentials = match state.credentials.lock() {
        Ok(credentials) => credentials,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponseEnvelope::new(ControlError::new(
                    ErrorCode::Internal,
                    "local-control credential broker is unavailable",
                ))),
            )
                .into_response();
        }
    };
    credentials.insert(auth_token.secret().to_owned(), grant.clone());
    Json(ScopedCredential {
        bearer_token: auth_token.secret().to_owned(),
        grant,
    })
    .into_response()
}

async fn handle_control_request(
    State(state): State<ControlServerState>,
    headers: HeaderMap,
    payload: Result<Json<RequestEnvelope>, JsonRejection>,
) -> Response {
    if let Err(error) = ensure_feature_enabled() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponseEnvelope::new(error)),
        )
            .into_response();
    }
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let auth_token = match AuthToken::from_authorization_header(auth_header) {
        Ok(token) => token,
        Err(error) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponseEnvelope::new(error)),
            )
                .into_response();
        }
    };
    let request = match payload {
        Ok(Json(request)) => request,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponseEnvelope::new(ControlError::with_details(
                    ErrorCode::InvalidRequest,
                    "failed to decode local-control request",
                    err.to_string(),
                ))),
            )
                .into_response();
        }
    };
    let grant = match state.credentials.lock() {
        Ok(credentials) => credentials.get(auth_token.secret()).cloned(),
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponseEnvelope::new(ControlError::new(
                    ErrorCode::Internal,
                    "local-control credential broker is unavailable",
                ))),
            )
                .into_response();
        }
    };
    let Some(grant) = grant else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponseEnvelope::new(ControlError::new(
                ErrorCode::UnauthorizedLocalClient,
                "local-control credential is invalid",
            ))),
        )
            .into_response();
    };
    let request_id = request.request_id;
    let response = match state
        .bridge_spawner
        .spawn(move |bridge, ctx| bridge.handle_request(request, grant, ctx))
        .await
    {
        Ok(response) => response,
        Err(_) => ResponseEnvelope::error(
            request_id,
            ControlError::new(
                ErrorCode::BridgeUnavailable,
                "local-control app bridge is unavailable",
            ),
        ),
    };
    let status = match &response.response {
        ControlResponse::Ok { .. } => StatusCode::OK,
        ControlResponse::Error { .. } => StatusCode::BAD_REQUEST,
    };
    (status, Json(response)).into_response()
}

fn validate_drive_target(target: &TargetSelector, action: ActionKind) -> Result<(), ControlError> {
    if target.window.is_some()
        || target.tab.is_some()
        || target.pane.is_some()
        || target.session.is_some()
        || target.block.is_some()
        || target.file.is_some()
    {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} does not accept window, tab, pane, session, block, or file selectors",
                action.as_str()
            ),
        ));
    }
    match (action, target.drive.as_ref()) {
        (ActionKind::DriveList, None) | (ActionKind::DriveGet, None) => Ok(()),
        (ActionKind::DriveGet, Some(DriveTarget::Id { id, .. })) => {
            if id.0.is_empty() {
                return Err(ControlError::new(
                    ErrorCode::InvalidSelector,
                    "drive.get requires a non-empty Drive object id selector",
                ));
            }
            Ok(())
        }
        (_, Some(DriveTarget::Name { .. })) => Err(ControlError::new(
            ErrorCode::UnsupportedAction,
            format!(
                "{} does not support Drive name selectors yet",
                action.as_str()
            ),
        )),
        _ => Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{} does not accept a Drive selector", action.as_str()),
        )),
    }
}

fn validate_tab_create_target(target: &TargetSelector) -> Result<(), ControlError> {
    if matches!(target.window.as_ref(), Some(WindowTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            "tab.create cannot resolve the requested window id",
        ));
    }
    if !matches!(target.window.as_ref(), None | Some(WindowTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create only supports the active window selector",
        ));
    }
    if matches!(target.tab.as_ref(), Some(TabTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            "tab.create cannot resolve the requested tab id",
        ));
    }
    if !matches!(target.tab.as_ref(), None | Some(TabTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create does not accept a concrete tab selector",
        ));
    }
    if matches!(target.pane.as_ref(), Some(PaneTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            "tab.create cannot resolve the requested pane id",
        ));
    }
    if !matches!(target.pane.as_ref(), None | Some(PaneTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create does not accept a concrete pane selector",
        ));
    }
    if target.session.is_some()
        || target.block.is_some()
        || target.file.is_some()
        || target.drive.is_some()
    {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create does not accept session, block, file, or drive selectors",
        ));
    }
    Ok(())
}

fn validate_instance_metadata_read_target(
    action: ActionKind,
    target: &TargetSelector,
) -> Result<(), ControlError> {
    if target.window.is_some()
        || target.tab.is_some()
        || target.pane.is_some()
        || target.session.is_some()
        || target.block.is_some()
        || target.file.is_some()
        || target.drive.is_some()
    {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} does not accept target selectors; it only reads state already represented in Warp",
                action.as_str()
            ),
        ));
    }
    Ok(())
}

fn validate_block_list_target(target: &TargetSelector) -> Result<(), ControlError> {
    validate_active_terminal_target(target, ActionKind::BlockList)?;
    if target.block.is_some() {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "block.list does not accept a block selector",
        ));
    }
    Ok(())
}

fn validate_block_get_target(target: &TargetSelector) -> Result<(), ControlError> {
    validate_active_terminal_target(target, ActionKind::BlockGet)?;
    if target.block.is_some() {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "block.get uses its block_id parameter instead of a block selector",
        ));
    }
    Ok(())
}

fn validate_active_terminal_target(
    target: &TargetSelector,
    action: ActionKind,
) -> Result<(), ControlError> {
    let action_name = action.as_str();
    if matches!(target.window.as_ref(), Some(WindowTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested window id"),
        ));
    }
    if !matches!(target.window.as_ref(), None | Some(WindowTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active window selector"),
        ));
    }
    if matches!(target.tab.as_ref(), Some(TabTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested tab id"),
        ));
    }
    if !matches!(target.tab.as_ref(), None | Some(TabTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active tab selector"),
        ));
    }
    if matches!(target.pane.as_ref(), Some(PaneTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested pane id"),
        ));
    }
    if !matches!(target.pane.as_ref(), None | Some(PaneTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active pane selector"),
        ));
    }
    if target.file.is_some() || target.drive.is_some() {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} does not accept file or drive selectors"),
        ));
    }
    Ok(())
}

fn validate_terminal_read_target(
    action: ActionKind,
    target: &TargetSelector,
) -> Result<(), ControlError> {
    let action_name = action.as_str();
    if matches!(target.window.as_ref(), Some(WindowTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested window id"),
        ));
    }
    if !matches!(target.window.as_ref(), None | Some(WindowTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active window selector"),
        ));
    }
    if matches!(target.tab.as_ref(), Some(TabTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested tab id"),
        ));
    }
    if !matches!(target.tab.as_ref(), None | Some(TabTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active tab selector"),
        ));
    }
    if matches!(target.pane.as_ref(), Some(PaneTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested pane id"),
        ));
    }
    if !matches!(target.pane.as_ref(), None | Some(PaneTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active pane selector"),
        ));
    }
    if matches!(target.session.as_ref(), Some(SessionTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{action_name} cannot resolve the requested session id"),
        ));
    }
    if !matches!(target.session.as_ref(), None | Some(SessionTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} only supports the active session selector"),
        ));
    }
    if target.block.is_some() || target.file.is_some() || target.drive.is_some() {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!("{action_name} does not accept block, file, or drive selectors"),
        ));
    }
    Ok(())
}

fn resolve_terminal_read_target(
    action: ActionKind,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<ResolvedTerminalTarget, ControlError> {
    validate_terminal_read_target(action, target)?;
    let window_id = require_active_window_id_for_action(ctx.windows().active_window(), action)?;
    if let Some(workspace) = ctx
        .views_of_type::<Workspace>(window_id)
        .and_then(|workspaces| workspaces.into_iter().next())
    {
        let pane_group = workspace.read(ctx, |workspace, _| {
            workspace.active_tab_pane_group().clone()
        });
        return resolve_terminal_in_pane_group(action, target, pane_group, ctx);
    }
    let terminal_view = ctx
        .views_of_type::<TerminalView>(window_id)
        .and_then(|terminals| terminals.into_iter().next())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                format!("{} requires a target terminal session", action.as_str()),
            )
        })?;
    Ok(ResolvedTerminalTarget { terminal_view })
}

fn resolve_terminal_in_pane_group(
    action: ActionKind,
    target: &TargetSelector,
    pane_group: ViewHandle<PaneGroup>,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<ResolvedTerminalTarget, ControlError> {
    let terminal_view = pane_group.read(ctx, |pane_group, ctx| {
        let pane_id = if matches!(target.pane, Some(PaneTarget::Active)) {
            pane_group.focused_pane_id(ctx)
        } else {
            pane_group
                .active_session_id(ctx)
                .map(PaneId::from)
                .ok_or_else(|| {
                    ControlError::new(
                        ErrorCode::MissingTarget,
                        format!("{} requires an active terminal session", action.as_str()),
                    )
                })?
        };
        let terminal_view = pane_group
            .terminal_view_from_pane_id(pane_id, ctx)
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::TargetStateConflict,
                    format!(
                        "{} target pane does not contain a terminal session",
                        action.as_str()
                    ),
                )
            })?;
        Ok::<_, ControlError>(terminal_view)
    })?;
    Ok(ResolvedTerminalTarget { terminal_view })
}

fn validate_action_params(action: &::local_control::Action) -> Result<(), ControlError> {
    match action.kind {
        ActionKind::ActionGet => {
            let params = action.params_as::<ActionGetParams>()?;
            action_metadata_for_name(&params.action)?;
            Ok(())
        }
        ActionKind::SettingGet => action.params_as::<SettingGetParams>().map(|_| ()),
        ActionKind::AppInspect
        | ActionKind::ActionList
        | ActionKind::TabCreate
        | ActionKind::ThemeList
        | ActionKind::AppearanceGet
        | ActionKind::SettingList
        | ActionKind::FileList
        | ActionKind::ProjectActive
        | ActionKind::ProjectList
        | ActionKind::InputGet => validate_empty_action_params(action),
        ActionKind::BlockList => action.params_as::<BlockListParams>().map(|_| ()),
        ActionKind::BlockGet => action.params_as::<BlockGetParams>().map(|_| ()),
        ActionKind::HistoryList => {
            let params = action.params.as_object().ok_or_else(|| {
                ControlError::new(
                    ErrorCode::InvalidParams,
                    "history.list parameters must be an object",
                )
            })?;
            if params.keys().all(|key| key == "limit") {
                action.params_as::<HistoryListParams>()?;
                Ok(())
            } else {
                Err(ControlError::new(
                    ErrorCode::InvalidParams,
                    "history.list only accepts an optional limit parameter",
                ))
            }
        }
        ActionKind::DriveList => action.params_as::<DriveListParams>().map(|_| ()),
        ActionKind::DriveGet => action.params_as::<DriveGetParams>().and_then(|params| {
            if params.id.is_empty() {
                return Err(ControlError::new(
                    ErrorCode::InvalidParams,
                    "drive.get requires a non-empty Drive object id",
                ));
            }
            Ok(())
        }),
        _ => Ok(()),
    }
}

fn validate_empty_action_params(action: &::local_control::Action) -> Result<(), ControlError> {
    if action
        .params
        .as_object()
        .is_some_and(serde_json::Map::is_empty)
    {
        return Ok(());
    }
    Err(ControlError::new(
        ErrorCode::InvalidParams,
        format!("{} does not accept parameters", action.kind.as_str()),
    ))
}

fn action_metadata_for_name(
    action_name: &str,
) -> Result<::local_control::ActionMetadata, ControlError> {
    ActionKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.as_str() == action_name)
        .map(ActionKind::metadata)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::NotAllowlisted,
                format!("{action_name} is not an allowlisted local-control action"),
            )
        })
}

fn open_file_summaries(ctx: &mut ModelContext<LocalControlBridge>) -> Vec<FileSummary> {
    let window_ids: Vec<_> = ctx.window_ids().collect();
    let mut files = Vec::new();
    for window_id in window_ids {
        let Some(code_views) = ctx.views_of_type::<CodeView>(window_id) else {
            continue;
        };
        for code_view in code_views {
            code_view.read(ctx, |code_view, _ctx| {
                for index in 0..code_view.tab_count() {
                    if let Some(location) = code_view.tab_at(index).and_then(|tab| tab.location()) {
                        files.push(FileSummary {
                            path: location.display_path(),
                            tab_id: None,
                        });
                    }
                }
            });
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

fn active_project_path(ctx: &mut ModelContext<LocalControlBridge>) -> Option<String> {
    let window_id = ctx.windows().active_window()?;
    let repo_path = ActiveSession::as_ref(ctx)
        .terminal_view_id(window_id)
        .and_then(|terminal_view_id| ctx.view_with_id::<TerminalView>(window_id, terminal_view_id))
        .and_then(|terminal| {
            terminal
                .as_ref(ctx)
                .current_repo_path()
                .map(|path| path.display_path())
        });
    repo_path.or_else(|| {
        ActiveSession::as_ref(ctx)
            .working_directory(window_id)
            .map(|path| path.display_path())
    })
}

fn active_project_summary(ctx: &mut ModelContext<LocalControlBridge>) -> Option<ProjectSummary> {
    active_project_path(ctx).map(|path| ProjectSummary {
        path,
        is_active: true,
        last_opened_at: None,
    })
}

fn project_summaries(ctx: &mut ModelContext<LocalControlBridge>) -> Vec<ProjectSummary> {
    let active_path = active_project_path(ctx);
    let mut projects = BTreeMap::new();
    ProjectManagementModel::handle(ctx).read(ctx, |model, _ctx| {
        for project in model.all_projects() {
            projects.insert(
                project.path.clone(),
                ProjectSummary {
                    path: project.path.clone(),
                    is_active: active_path.as_ref() == Some(&project.path),
                    last_opened_at: project
                        .last_opened_ts
                        .map(|timestamp| timestamp.to_string()),
                },
            );
        }
    });
    if let Some(path) = active_path {
        projects.entry(path.clone()).or_insert(ProjectSummary {
            path,
            is_active: true,
            last_opened_at: None,
        });
    }
    projects.into_values().collect()
}

fn to_control_data<T: serde::Serialize>(data: T) -> Result<serde_json::Value, ControlError> {
    serde_json::to_value(data).map_err(|err| {
        ControlError::with_details(
            ErrorCode::Internal,
            "failed to encode local-control response",
            err.to_string(),
        )
    })
}
fn warp_control_cli_enabled() -> bool {
    FeatureFlag::WarpControlCli.is_enabled()
}

fn ensure_feature_enabled() -> Result<(), ControlError> {
    if warp_control_cli_enabled() {
        return Ok(());
    }
    Err(ControlError::new(
        ErrorCode::LocalControlDisabled,
        "Warp control CLI is disabled by feature flag",
    ))
}

fn target_window_id(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<warpui::WindowId, ControlError> {
    require_active_window_id(ctx.windows().active_window())
}

fn require_active_window_id(
    active_window: Option<warpui::WindowId>,
) -> Result<warpui::WindowId, ControlError> {
    require_active_window_id_for_action(active_window, ActionKind::TabCreate)
}

fn require_active_window_id_for_action(
    active_window: Option<warpui::WindowId>,
    action: ActionKind,
) -> Result<warpui::WindowId, ControlError> {
    active_window.ok_or_else(|| {
        ControlError::new(
            ErrorCode::MissingTarget,
            format!("{} requires an active Warp window", action.as_str()),
        )
    })
}
fn target_terminal_view(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<ViewHandle<TerminalView>, ControlError> {
    let window_id = target_window_id(ctx)?;
    let workspace = ctx
        .views_of_type::<Workspace>(window_id)
        .and_then(|workspaces| workspaces.into_iter().next())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                "block read requires a workspace in the target window",
            )
        })?;
    workspace
        .read(ctx, |workspace, ctx| {
            workspace
                .active_tab_pane_group()
                .read(ctx, |pane_group, ctx| pane_group.active_session_view(ctx))
        })
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                "block read requires an active terminal session",
            )
        })
}

fn resolve_session_selector(
    target: Option<&SessionTarget>,
    active_session_id: Option<SessionId>,
    action: ActionKind,
) -> Result<SessionId, ControlError> {
    match target {
        None | Some(SessionTarget::Active) => active_session_id.ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                format!("{} requires an active terminal session", action.as_str()),
            )
        }),
        Some(SessionTarget::Id { id }) => id.0.parse::<u64>().map(SessionId::from).map_err(|err| {
            ControlError::with_details(
                ErrorCode::InvalidSelector,
                format!("{} received an invalid session id", action.as_str()),
                err.to_string(),
            )
        }),
    }
}

fn block_summary(
    block: &crate::terminal::model::block::Block,
    index: usize,
) -> Option<BlockSummary> {
    let session_id = block.session_id()?;
    let command = block.command_to_string();
    Some(BlockSummary {
        block_id: block.id().to_string(),
        session_id: session_id.as_u64().to_string(),
        index: index as u32,
        command: (!command.is_empty()).then_some(command),
    })
}

fn block_list_result_from_model(
    model: &TerminalModel,
    session_id: SessionId,
    explicit_session: bool,
    params: BlockListParams,
) -> Result<BlockListResult, ControlError> {
    let mut blocks: Vec<BlockSummary> = model
        .block_list()
        .blocks()
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let summary = block_summary(block, index)?;
            (summary.session_id == session_id.as_u64().to_string()).then_some(summary)
        })
        .collect();
    if explicit_session && blocks.is_empty() {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            "block.list cannot resolve the requested session id",
        ));
    }
    if let Some(limit) = params.limit {
        let start = blocks.len().saturating_sub(limit as usize);
        blocks = blocks.split_off(start);
    }
    Ok(BlockListResult { blocks })
}

fn block_get_result_from_model(
    model: &TerminalModel,
    session_id: SessionId,
    block_id: &str,
) -> Result<BlockGetResult, ControlError> {
    model
        .block_list()
        .blocks()
        .iter()
        .enumerate()
        .find_map(|(index, block)| {
            if block.id().as_str() != block_id || block.session_id() != Some(session_id) {
                return None;
            }
            block_summary(block, index).map(|summary| BlockGetResult {
                block: summary,
                output: Some(block.output_to_string()),
            })
        })
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::StaleTarget,
                "block.get cannot resolve the requested block id",
            )
        })
}

fn outside_warp_any_implemented_action_enabled(ctx: &ModelContext<LocalControlServer>) -> bool {
    let settings = LocalControlSettings::as_ref(ctx);
    ActionKind::implemented_metadata()
        .into_iter()
        .any(|metadata| {
            outside_warp_permission_enabled_for_settings(settings, metadata.permission_category)
        })
}

#[cfg(test)]
fn outside_warp_action_enabled_for_settings(
    settings: &LocalControlSettings,
    action: ActionKind,
) -> bool {
    outside_warp_permission_enabled_for_settings(settings, action.metadata().permission_category)
}

fn outside_warp_permission_enabled_for_settings(
    settings: &LocalControlSettings,
    permission: PermissionCategory,
) -> bool {
    let context = LocalControlInvocationContext::OutsideWarp;
    settings.is_context_enabled(context)
        && settings.is_permission_enabled(context, local_permission(permission))
}

#[cfg(test)]
fn capabilities() -> Vec<ActionKind> {
    ActionKind::implemented_metadata()
        .into_iter()
        .map(|metadata| metadata.kind)
        .collect()
}

fn local_invocation_context(context: InvocationContext) -> LocalControlInvocationContext {
    match context {
        InvocationContext::InsideWarp => LocalControlInvocationContext::InsideWarp,
        InvocationContext::OutsideWarp => LocalControlInvocationContext::OutsideWarp,
    }
}

fn local_permission(permission: PermissionCategory) -> LocalControlPermissionCategory {
    match permission {
        PermissionCategory::ReadMetadata => LocalControlPermissionCategory::MetadataReads,
        PermissionCategory::ReadUnderlyingData => {
            LocalControlPermissionCategory::UnderlyingDataReads
        }
        PermissionCategory::MutateAppState => LocalControlPermissionCategory::AppStateMutations,
        PermissionCategory::MutateMetadataConfiguration => {
            LocalControlPermissionCategory::MetadataConfigurationMutations
        }
        PermissionCategory::MutateUnderlyingData => {
            LocalControlPermissionCategory::UnderlyingDataMutations
        }
    }
}

fn ensure_action_allowed(
    context: InvocationContext,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    let settings = LocalControlSettings::as_ref(ctx);
    ensure_settings_allow_action(settings, context, action)
}

fn ensure_settings_allow_action(
    settings: &LocalControlSettings,
    context: InvocationContext,
    action: ActionKind,
) -> Result<(), ControlError> {
    let context = local_invocation_context(context);
    if !settings.is_context_enabled(context) {
        return Err(ControlError::new(
            ErrorCode::LocalControlDisabled,
            "local control is disabled for this invocation context",
        ));
    }
    let permission = local_permission(action.metadata().permission_category);
    if !settings.is_permission_enabled(context, permission) {
        return Err(ControlError::new(
            ErrorCode::InsufficientPermissions,
            format!(
                "{} requires a local-control permission that is disabled",
                action.as_str()
            ),
        ));
    }
    Ok(())
}

fn authenticated_user_subject_for_action(
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Option<String>, ControlError> {
    if !action.metadata().requires_authenticated_user {
        return Ok(None);
    }
    authenticated_user_subject(ctx).map(Some)
}

fn ensure_authenticated_user_matches(
    grant: &CredentialGrant,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<(), ControlError> {
    if !grant.authenticated_user.required {
        return Ok(());
    }
    let subject = authenticated_user_subject(ctx)?;
    if grant.authenticated_user.subject.as_deref() != Some(subject.as_str()) {
        return Err(ControlError::new(
            ErrorCode::AuthenticatedUserUnavailable,
            "the authenticated Warp user no longer matches the credential grant",
        ));
    }
    Ok(())
}

fn authenticated_user_subject(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<String, ControlError> {
    let auth_state = AuthStateProvider::as_ref(ctx).get();
    if auth_state.is_anonymous_or_logged_out() {
        return Err(ControlError::new(
            ErrorCode::AuthenticatedUserUnavailable,
            "this action requires a logged-in Warp user",
        ));
    }
    auth_state
        .user_id()
        .map(|uid| uid.as_string())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::AuthenticatedUserUnavailable,
                "this action requires a logged-in Warp user",
            )
        })
}

fn drive_object_summary(object: &dyn CloudObject) -> Option<DriveObjectSummary> {
    Some(DriveObjectSummary {
        object_type: control_drive_object_type(object)?,
        id: object.uid(),
        name: object.display_name(),
    })
}

fn drive_object_get_result(
    object: &dyn CloudObject,
    requested_type: ControlDriveObjectType,
) -> Result<DriveGetResult, ControlError> {
    let summary = drive_object_summary(object).ok_or_else(|| {
        ControlError::new(
            ErrorCode::UnsupportedAction,
            "drive.get does not support this Drive object type",
        )
    })?;
    if summary.object_type != requested_type {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            "drive.get Drive object type does not match the requested type",
        ));
    }
    Ok(DriveGetResult {
        object: summary,
        content: drive_object_content(object, requested_type)?,
    })
}

fn control_drive_object_type(object: &dyn CloudObject) -> Option<ControlDriveObjectType> {
    match object.object_type() {
        ObjectType::Workflow => {
            let workflow = object.as_any().downcast_ref::<CloudWorkflow>()?;
            if workflow.model().data.is_agent_mode_workflow() {
                Some(ControlDriveObjectType::Prompt)
            } else {
                Some(ControlDriveObjectType::Workflow)
            }
        }
        ObjectType::Notebook => Some(ControlDriveObjectType::Notebook),
        ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
            JsonObjectType::EnvVarCollection,
        )) => Some(ControlDriveObjectType::Environment),
        _ => None,
    }
}

fn drive_object_content(
    object: &dyn CloudObject,
    object_type: ControlDriveObjectType,
) -> Result<serde_json::Value, ControlError> {
    match object_type {
        ControlDriveObjectType::Workflow | ControlDriveObjectType::Prompt => object
            .as_any()
            .downcast_ref::<CloudWorkflow>()
            .ok_or_else(drive_type_mismatch_error)
            .and_then(|workflow| {
                serde_json::to_value(&workflow.model().data).map_err(json_response_error)
            }),
        ControlDriveObjectType::Notebook => {
            let notebook = object
                .as_any()
                .downcast_ref::<CloudNotebook>()
                .ok_or_else(drive_type_mismatch_error)?;
            Ok(json!({
                "title": notebook.model().title.clone(),
                "data": notebook.model().data.clone(),
                "ai_document_id": notebook.model().ai_document_id.as_ref().map(|id| id.to_string()),
                "conversation_id": notebook.model().conversation_id.clone(),
            }))
        }
        ControlDriveObjectType::Environment => object
            .as_any()
            .downcast_ref::<CloudEnvVarCollection>()
            .ok_or_else(drive_type_mismatch_error)
            .and_then(|env_var_collection| {
                serde_json::to_value(&env_var_collection.model().string_model)
                    .map_err(json_response_error)
            }),
    }
}

fn drive_object_type_rank(object_type: ControlDriveObjectType) -> u8 {
    match object_type {
        ControlDriveObjectType::Workflow => 0,
        ControlDriveObjectType::Notebook => 1,
        ControlDriveObjectType::Environment => 2,
        ControlDriveObjectType::Prompt => 3,
    }
}

fn drive_type_mismatch_error() -> ControlError {
    ControlError::new(
        ErrorCode::TargetStateConflict,
        "drive.get Drive object type does not match the requested type",
    )
}

fn json_response_error(error: serde_json::Error) -> ControlError {
    ControlError::with_details(
        ErrorCode::Internal,
        "failed to encode local-control Drive response",
        error.to_string(),
    )
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
