use crate::code::view::CodeView;
use crate::features::FeatureFlag;
use crate::projects::ProjectManagementModel;
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use crate::settings::{
    LocalControlInvocationContext, LocalControlPermissionCategory, LocalControlSettings,
};
use crate::terminal::view::TerminalView;
use crate::workspace::ActiveSession;
use ::local_control::auth::{CredentialGrant, CredentialRequest, ScopedCredential};
use ::local_control::protocol::{
    ActionGetParams, FileListResult, FileSummary, PaneTarget, ProjectActiveResult,
    ProjectListResult, ProjectSummary, TabTarget, TargetSelector, WindowTarget,
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
use warp_core::channel::ChannelState;
use warpui::{Entity, ModelContext, ModelSpawner, SingletonEntity, TypedActionView};

use crate::workspace::{Workspace, WorkspaceAction};

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
    let settings_check = state
        .bridge_spawner
        .spawn({
            let action = request.action;
            let invocation_context = request.invocation_context;
            move |_, ctx| ensure_action_allowed(invocation_context, action, ctx)
        })
        .await;
    match settings_check {
        Ok(Ok(())) => {}
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
    }
    let auth_token = AuthToken::generate();
    let grant = CredentialGrant::new(
        state.instance_id.clone(),
        request.action,
        request.invocation_context,
        Duration::minutes(5),
    );
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
fn validate_action_params(action: &::local_control::Action) -> Result<(), ControlError> {
    match action.kind {
        ActionKind::ActionGet => {
            let params = action.params_as::<ActionGetParams>()?;
            action_metadata_for_name(&params.action)?;
            Ok(())
        }
        ActionKind::AppInspect
        | ActionKind::ActionList
        | ActionKind::TabCreate
        | ActionKind::FileList
        | ActionKind::ProjectActive
        | ActionKind::ProjectList => validate_empty_action_params(action),
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
    active_window.ok_or_else(|| {
        ControlError::new(
            ErrorCode::MissingTarget,
            "tab.create requires an active Warp window",
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
