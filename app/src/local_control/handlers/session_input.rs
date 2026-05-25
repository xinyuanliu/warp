//! Session activation/cycling/reopen and input mutation handlers.
use ::local_control::protocol::{
    InputInsertParams, InputMode, InputModeSetParams, InputReplaceParams, InputStateResult,
    SessionTarget, TargetSelector, WindowTarget,
};
use ::local_control::{ActionKind, ControlError, ErrorCode};
use serde_json::json;
use warpui::{ModelContext, TypedActionView, ViewContext, ViewHandle};

use crate::local_control::resolver::require_active_window_id_for_action;
use crate::local_control::LocalControlBridge;
use crate::pane_group::{PaneGroup, PaneId};
use crate::terminal::input::Input;
use crate::terminal::model::session::SessionId;
use crate::terminal::view::TerminalView;
use crate::workspace::{Workspace, WorkspaceAction};

use super::data::resolve_terminal_read_target;

pub(crate) fn validate_session_cycle_target(
    action: ActionKind,
    target: &TargetSelector,
) -> Result<(), ControlError> {
    if matches!(target.window.as_ref(), Some(WindowTarget::Id { .. })) {
        return Err(ControlError::new(
            ErrorCode::StaleTarget,
            format!("{} cannot resolve the requested window id", action.as_str()),
        ));
    }
    if !matches!(target.window.as_ref(), None | Some(WindowTarget::Active)) {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} only supports the active window selector",
                action.as_str()
            ),
        ));
    }
    if action != ActionKind::SessionActivate
        && matches!(target.session.as_ref(), Some(SessionTarget::Id { .. }))
    {
        return Err(ControlError::new(
            ErrorCode::InvalidSelector,
            format!(
                "{} does not accept a concrete session selector",
                action.as_str()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn cycle_session(
    action: ActionKind,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    validate_session_cycle_target(action, target)?;
    let window_id = require_active_window_id_for_action(ctx.windows().active_window(), action)?;
    let workspace = ctx
        .views_of_type::<Workspace>(window_id)
        .and_then(|workspaces| workspaces.into_iter().next())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                format!(
                    "{} requires a workspace in the target window",
                    action.as_str()
                ),
            )
        })?;
    let (previous_tab_index, active_tab_index, tab_count) =
        workspace.update(ctx, |workspace, ctx| {
            let previous_tab_index = workspace.active_tab_index();
            let workspace_action = match action {
                ActionKind::PaneSessionPrevious | ActionKind::SessionPrevious => {
                    WorkspaceAction::CyclePrevSession
                }
                ActionKind::PaneSessionNext | ActionKind::SessionNext => {
                    WorkspaceAction::CycleNextSession
                }
                _ => {
                    return (
                        previous_tab_index,
                        workspace.active_tab_index(),
                        workspace.tab_count(),
                    );
                }
            };
            workspace.handle_action(&workspace_action, ctx);
            (
                previous_tab_index,
                workspace.active_tab_index(),
                workspace.tab_count(),
            )
        });
    Ok(json!({
        "action": action.as_str(),
        "window_id": window_id.to_string(),
        "previous_tab_index": previous_tab_index,
        "active_tab_index": active_tab_index,
        "tab_count": tab_count,
    }))
}

pub(crate) fn activate_session(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let action = ActionKind::SessionActivate;
    validate_session_cycle_target(action, target)?;
    let window_id = require_active_window_id_for_action(ctx.windows().active_window(), action)?;
    let workspace = ctx
        .views_of_type::<Workspace>(window_id)
        .and_then(|workspaces| workspaces.into_iter().next())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                "session.activate requires a workspace in the target window",
            )
        })?;
    let pane_group = workspace.read(ctx, |workspace, _| {
        workspace.active_tab_pane_group().clone()
    });
    let tab_id = pane_group.id().to_string();
    let pane_id = pane_group.update(ctx, |pane_group, ctx| {
        let pane_id = resolve_target_session_pane_id(pane_group, target, action, ctx)?;
        if pane_group.terminal_view_from_pane_id(pane_id, ctx).is_none() {
            return Err(ControlError::new(
                ErrorCode::TargetStateConflict,
                "session.activate target pane does not contain a terminal session",
            ));
        }
        pane_group.focus_pane_by_id(pane_id, ctx);
        Ok::<_, ControlError>(pane_id)
    })?;
    Ok(json!({
        "action": action.as_str(),
        "window_id": window_id.to_string(),
        "tab_id": tab_id,
        "pane_id": pane_id.to_string(),
        "session_id": pane_id.to_string(),
    }))
}

pub(crate) fn reopen_session(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    validate_session_cycle_target(ActionKind::SessionReopen, target)?;
    let window_id = require_active_window_id_for_action(
        ctx.windows().active_window(),
        ActionKind::SessionReopen,
    )?;
    let workspace = ctx
        .views_of_type::<Workspace>(window_id)
        .and_then(|workspaces| workspaces.into_iter().next())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                "session.reopen requires a workspace in the target window",
            )
        })?;
    workspace.update(ctx, |workspace, ctx| {
        workspace.handle_action(&WorkspaceAction::ReopenClosedSession, ctx);
    });
    Ok(json!({
        "action": ActionKind::SessionReopen.as_str(),
        "handled": true,
        "window_id": window_id.to_string(),
    }))
}

fn resolve_target_session_pane_id(
    pane_group: &PaneGroup,
    target: &TargetSelector,
    action: ActionKind,
    ctx: &mut ViewContext<PaneGroup>,
) -> Result<PaneId, ControlError> {
    match target.session.as_ref() {
        None | Some(SessionTarget::Active) => {
            pane_group
                .active_session_id(ctx)
                .map(PaneId::from)
                .ok_or_else(|| {
                    ControlError::new(
                        ErrorCode::MissingTarget,
                        format!("{} requires an active terminal session", action.as_str()),
                    )
                })
        }
        Some(SessionTarget::Id { id }) => {
            pane_group
                .visible_pane_ids()
                .into_iter()
                .find(|pane_id| pane_id.to_string() == id.0)
                .ok_or_else(|| {
                    ControlError::new(
                        ErrorCode::StaleTarget,
                        format!("{} cannot resolve the requested session id", action.as_str()),
                    )
                })
        }
    }
}

pub(crate) fn insert_input(
    target: &TargetSelector,
    params: InputInsertParams,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let resolved = resolve_terminal_read_target(ActionKind::InputInsert, target, ctx)?;
    let session_id =
        active_session_id_for_terminal(&resolved.terminal_view, ActionKind::InputInsert, ctx)?;
    let input = resolved
        .terminal_view
        .read(ctx, |terminal: &TerminalView, _| terminal.input().clone());
    if params.replace {
        input.update(ctx, |input: &mut crate::terminal::input::Input, ctx| {
            input.replace_buffer_content(&params.text, ctx);
        });
    } else {
        input.update(ctx, |input: &mut crate::terminal::input::Input, ctx| {
            input.append_to_buffer(&params.text, ctx);
        });
    }
    input_state_result_for_input(input, session_id, ctx).and_then(to_control_data)
}

pub(crate) fn replace_input(
    target: &TargetSelector,
    params: InputReplaceParams,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let resolved = resolve_terminal_read_target(ActionKind::InputReplace, target, ctx)?;
    let session_id =
        active_session_id_for_terminal(&resolved.terminal_view, ActionKind::InputReplace, ctx)?;
    let input = resolved
        .terminal_view
        .read(ctx, |terminal: &TerminalView, _| terminal.input().clone());
    input.update(ctx, |input: &mut crate::terminal::input::Input, ctx| {
        input.replace_buffer_content(&params.text, ctx);
    });
    input_state_result_for_input(input, session_id, ctx).and_then(to_control_data)
}

pub(crate) fn clear_input(
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let resolved = resolve_terminal_read_target(ActionKind::InputClear, target, ctx)?;
    let session_id =
        active_session_id_for_terminal(&resolved.terminal_view, ActionKind::InputClear, ctx)?;
    let input = resolved
        .terminal_view
        .read(ctx, |terminal: &TerminalView, _| terminal.input().clone());
    input.update(ctx, |input: &mut crate::terminal::input::Input, ctx| {
        input.clear_buffer_and_reset_undo_stack(ctx);
    });
    input_state_result_for_input(input, session_id, ctx).and_then(to_control_data)
}

pub(crate) fn set_input_mode(
    target: &TargetSelector,
    params: InputModeSetParams,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    let resolved = resolve_terminal_read_target(ActionKind::InputModeSet, target, ctx)?;
    let session_id =
        active_session_id_for_terminal(&resolved.terminal_view, ActionKind::InputModeSet, ctx)?;
    let input = resolved
        .terminal_view
        .read(ctx, |terminal: &TerminalView, _| terminal.input().clone());
    input.update(
        ctx,
        |input: &mut crate::terminal::input::Input, ctx| match params.mode {
            InputMode::Terminal => input.set_input_mode_terminal(false, ctx),
            InputMode::Agent => input.set_input_mode_agent(false, ctx),
        },
    );
    input_state_result_for_input(input, session_id, ctx).and_then(to_control_data)
}

fn active_session_id_for_terminal(
    terminal_view: &ViewHandle<TerminalView>,
    action: ActionKind,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<SessionId, ControlError> {
    terminal_view
        .read(ctx, |terminal, _| terminal.active_block_session_id())
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::MissingTarget,
                format!("{} requires a target terminal session", action.as_str()),
            )
        })
}

fn input_state_result_for_input(
    input: ViewHandle<Input>,
    session_id: SessionId,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<InputStateResult, ControlError> {
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
    Ok(InputStateResult {
        session_id: session_id.as_u64().to_string(),
        text,
        cursor_offset,
    })
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
