use std::net::SocketAddr;

use ::local_control::{
    ActionKind, AuthToken, ControlEndpoint, ControlError, ControlResponse, ErrorCode,
    ErrorResponseEnvelope, InstanceId, InstanceRecord, RegisteredInstance, RequestEnvelope,
    ResponseEnvelope, PROTOCOL_VERSION,
};
use ::local_control::protocol::{PaneTarget, TabTarget, TargetSelector, WindowTarget};
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use warp_core::channel::ChannelState;
use warpui::{Entity, ModelContext, ModelSpawner, SingletonEntity, TypedActionView};

use crate::workspace::{Workspace, WorkspaceAction};

#[derive(Clone)]
struct ControlServerState {
    auth_token: AuthToken,
    bridge_spawner: ModelSpawner<LocalControlBridge>,
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
        let auth_token = AuthToken::generate();
        let record = InstanceRecord::for_current_process(
            ControlEndpoint::localhost(port.port()),
            &auth_token,
            ChannelState::channel().to_string(),
            ChannelState::app_id().to_string(),
            ChannelState::app_version().map(str::to_owned),
            capabilities(),
        );
        let bridge_spawner = LocalControlBridge::handle(ctx).update(ctx, |bridge, ctx| {
            bridge.set_instance_id(record.instance_id.clone());
            ctx.spawner()
        });
        let registered_instance = RegisteredInstance::register(record)?;
        let state = ControlServerState {
            auth_token,
            bridge_spawner,
        };
        let router = Router::new()
            .route("/v1/control", post(handle_control_request))
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
        ctx: &mut ModelContext<Self>,
    ) -> ResponseEnvelope {
        if request.protocol_version != PROTOCOL_VERSION {
            return ResponseEnvelope::error(
                request.request_id,
                ControlError::new(
                    ErrorCode::ProtocolVersionUnsupported,
                    format!("unsupported protocol version {}", request.protocol_version),
                ),
            );
        }
        match request.action.kind {
            ActionKind::AppPing => ResponseEnvelope::ok(
                request.request_id,
                json!({
                    "ok": true,
                    "pid": std::process::id(),
                    "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
                }),
            ),
            ActionKind::AppInspect => ResponseEnvelope::ok(
                request.request_id,
                json!({
                    "pid": std::process::id(),
                    "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
                    "channel": ChannelState::channel().to_string(),
                    "app_id": ChannelState::app_id().to_string(),
                    "app_version": ChannelState::app_version(),
                    "window_count": ctx.window_ids().count(),
                    "has_active_window": ctx.windows().active_window().is_some(),
                }),
            ),
            ActionKind::AppVersion => ResponseEnvelope::ok(
                request.request_id,
                json!({
                    "channel": ChannelState::channel().to_string(),
                    "app_id": ChannelState::app_id().to_string(),
                    "app_version": ChannelState::app_version(),
                    "protocol_version": PROTOCOL_VERSION,
                }),
            ),
            ActionKind::AppActive => ResponseEnvelope::ok(
                request.request_id,
                json!({
                    "instance_id": self.instance_id.as_ref().map(|id| id.0.as_str()),
                    "window_count": ctx.window_ids().count(),
                    "has_active_window": ctx.windows().active_window().is_some(),
                }),
            ),
            ActionKind::WindowList => ResponseEnvelope::ok(
                request.request_id,
                json!({
                    "window_count": ctx.window_ids().count(),
                    "has_active_window": ctx.windows().active_window().is_some(),
                }),
            ),
            ActionKind::TabList | ActionKind::PaneList | ActionKind::SessionList => {
                ResponseEnvelope::ok(
                    request.request_id,
                    json!({
                        "items": [],
                        "enumeration": "not_yet_implemented",
                    }),
                )
            }
            ActionKind::TabCreate => match self.create_terminal_tab(&request.target, ctx) {
                Ok(data) => ResponseEnvelope::ok(request.request_id, data),
                Err(error) => ResponseEnvelope::error(request.request_id, error),
            },
            ActionKind::SettingList => ResponseEnvelope::ok(
                request.request_id,
                json!({
                    "settings": [],
                    "enumeration": "not_yet_implemented",
                }),
            ),
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
        let window_id = ctx.windows().active_window().ok_or_else(|| {
            ControlError::new(
                ErrorCode::InvalidSelector,
                "tab.create requires an active Warp window",
            )
        })?;
        let workspace = ctx
            .views_of_type::<Workspace>(window_id)
            .and_then(|workspaces| workspaces.into_iter().next())
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::InvalidSelector,
                    "tab.create could not resolve an active workspace",
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
}

async fn handle_control_request(
    State(state): State<ControlServerState>,
    headers: HeaderMap,
    payload: Result<Json<RequestEnvelope>, JsonRejection>,
) -> Response {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if let Err(error) = state.auth_token.verify_authorization_header(auth_header) {
        return (
            StatusCode::UNAUTHORIZED,
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
                    "failed to decode local-control request",
                    err.to_string(),
                ))),
            )
                .into_response();
        }
    };
    let request_id = request.request_id;
    let response = match state
        .bridge_spawner
        .spawn(move |bridge, ctx| bridge.handle_request(request, ctx))
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
    if !matches!(target.window.as_ref(), None | Some(WindowTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create only supports the active window selector",
        ));
    }
    if !matches!(target.tab.as_ref(), None | Some(TabTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create does not accept a concrete tab selector",
        ));
    }
    if !matches!(target.pane.as_ref(), None | Some(PaneTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            "tab.create does not accept a concrete pane selector",
        ));
    }
    Ok(())
}

fn capabilities() -> Vec<ActionKind> {
    vec![
        ActionKind::AppPing,
        ActionKind::AppInspect,
        ActionKind::AppVersion,
        ActionKind::AppActive,
        ActionKind::AppFocus,
        ActionKind::AppSettingsOpen,
        ActionKind::AppCommandPaletteOpen,
        ActionKind::AppCommandSearchOpen,
        ActionKind::AppWarpDriveOpen,
        ActionKind::AppWarpDriveToggle,
        ActionKind::AppResourceCenterToggle,
        ActionKind::AppAiAssistantToggle,
        ActionKind::AppCodeReviewToggle,
        ActionKind::AppVerticalTabsToggle,
        ActionKind::WindowList,
        ActionKind::WindowCreate,
        ActionKind::WindowFocus,
        ActionKind::WindowClose,
        ActionKind::TabList,
        ActionKind::TabCreate,
        ActionKind::TabActivate,
        ActionKind::TabMove,
        ActionKind::TabRename,
        ActionKind::TabClose,
        ActionKind::PaneList,
        ActionKind::PaneSplit,
        ActionKind::PaneFocus,
        ActionKind::PaneNavigate,
        ActionKind::PaneClose,
        ActionKind::PaneMaximize,
        ActionKind::PaneResize,
        ActionKind::PaneSessionPrevious,
        ActionKind::PaneSessionNext,
        ActionKind::SessionList,
        ActionKind::InputInsert,
        ActionKind::InputReplace,
        ActionKind::InputClear,
        ActionKind::InputModeSet,
        ActionKind::ThemeList,
        ActionKind::ThemeSet,
        ActionKind::AppearanceGet,
        ActionKind::AppearanceSet,
        ActionKind::AppearanceFontSize,
        ActionKind::AppearanceZoom,
        ActionKind::SettingGet,
        ActionKind::SettingList,
        ActionKind::SettingSet,
        ActionKind::SettingToggle,
    ]
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
