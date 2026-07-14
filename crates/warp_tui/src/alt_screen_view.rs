//! Full-screen alt-screen rendering + raw input forwarding for the TUI.
//!
//! When a PTY app switches to the alternate screen (vim, htop, less, …), the
//! terminal model flips [`TerminalModel::is_alt_screen_active`] and populates a
//! dedicated alt-screen grid. [`TuiTerminalSessionView`] then renders this
//! element full-area instead of the block/transcript UI, and forwards
//! input straight to the PTY as escape sequences — mirroring the GUI's
//! `AltScreenElement` (`app/src/terminal/alt_screen/alt_screen_element.rs`).
//!
//! Covers rendering, the cursor, keyboard and SGR mouse forwarding, and
//! propagating the laid-out cell dimensions to the terminal model and PTY.
//!
//! [`TuiTerminalSessionView`]: crate::terminal_session_view::TuiTerminalSessionView
//! [`TerminalModel::is_alt_screen_active`]: warp::tui_export::TerminalModel

use std::ops::Deref as _;
use std::sync::Arc;

use async_channel::Sender;
use parking_lot::FairMutex;
use warp::tui_export::{KeystrokeWithDetails, TermMode, TerminalModel, ToEscapeSequence as _};
use warp_terminal::model::escape_sequences::{alt_screen_scroll_to_pty_bytes, ModeProvider};
use warp_terminal::model::grid::Dimensions as _;
use warp_terminal::model::mouse::{MouseAction, MouseButton, MouseState};
use warp_terminal::model::Point;
use warpui_core::elements::tui::{
    TuiBuffer, TuiConstraint, TuiElement, TuiEvent, TuiEventContext, TuiLayoutContext,
    TuiPaintContext, TuiRect, TuiRectExt as _, TuiSize,
};
use warpui_core::AppContext;

use crate::terminal_block::render_grid_handler;
use crate::terminal_session_view::TuiTerminalSessionAction;

/// Renders the terminal's alt-screen grid full-area and forwards input to the
/// PTY while a full-screen app is active.
pub(crate) struct AltScreenElement {
    model: Arc<FairMutex<TerminalModel>>,
    resize_tx: Sender<TuiSize>,
}

impl AltScreenElement {
    pub(crate) fn new(model: Arc<FairMutex<TerminalModel>>, resize_tx: Sender<TuiSize>) -> Self {
        Self { model, resize_tx }
    }
}

/// Converts a supported pointer event into the terminal's SGR mouse model.
fn mouse_state_for_event(
    event: &TuiEvent,
    area: TuiRect,
    is_mode_set: impl Fn(TermMode) -> bool,
) -> Option<MouseState> {
    if !is_mode_set(TermMode::SGR_MOUSE) {
        return None;
    }
    let reports_clicks = is_mode_set(TermMode::MOUSE_REPORT_CLICK);
    let reports_drag = is_mode_set(TermMode::MOUSE_DRAG);
    let reports_motion = is_mode_set(TermMode::MOUSE_MOTION);
    let reports_clicks = reports_clicks || reports_drag || reports_motion;
    let position = event.position()?;
    if !area.contains_point(position) {
        return None;
    }
    let point = Point::new(
        usize::from(position.y - area.y),
        usize::from(position.x - area.x),
    );

    let state = match event {
        TuiEvent::LeftMouseDown { modifiers, .. } if reports_clicks && !modifiers.shift => {
            MouseState::new(MouseButton::Left, MouseAction::Pressed, *modifiers)
        }
        TuiEvent::RightMouseDown { modifiers, .. } if reports_clicks && !modifiers.shift => {
            MouseState::new(MouseButton::Right, MouseAction::Pressed, *modifiers)
        }
        TuiEvent::LeftMouseUp { modifiers, .. } if reports_clicks && !modifiers.shift => {
            MouseState::new(MouseButton::Left, MouseAction::Released, *modifiers)
        }
        TuiEvent::LeftMouseDragged { modifiers, .. }
            if (reports_drag || reports_motion) && !modifiers.shift =>
        {
            MouseState::new(MouseButton::LeftDrag, MouseAction::Pressed, *modifiers)
        }
        TuiEvent::MouseMoved {
            modifiers,
            is_synthetic: false,
            ..
        } if reports_motion => MouseState::new(MouseButton::Move, MouseAction::Pressed, *modifiers),
        _ => return None,
    };
    Some(state.set_point(point))
}

/// Encodes a supported pointer event for the active alt-screen application.
fn mouse_event_to_pty_bytes<T: ModeProvider>(
    event: &TuiEvent,
    area: TuiRect,
    is_mode_set: impl Fn(TermMode) -> bool,
    mode_provider: &T,
) -> Option<Vec<u8>> {
    if let TuiEvent::ScrollWheel {
        position,
        delta: (_, rows),
        ..
    } = event
    {
        if !area.contains_point(*position) {
            return None;
        }
        let point = Point::new(
            usize::from(position.y - area.y),
            usize::from(position.x - area.x),
        );
        return alt_screen_scroll_to_pty_bytes(
            i32::try_from(*rows).ok()?,
            point,
            is_mode_set(TermMode::SGR_MOUSE),
            mode_provider,
        );
    }

    mouse_state_for_event(event, area, is_mode_set)
        .and_then(|state| state.to_escape_sequence(mode_provider))
}

impl TuiElement for AltScreenElement {
    fn layout(
        &mut self,
        constraint: TuiConstraint,
        _ctx: &mut TuiLayoutContext,
        _app: &AppContext,
    ) -> TuiSize {
        // The alt-screen app owns the whole pane.
        let size = constraint.max;
        let _ = self.resize_tx.try_send(size);
        size
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer, _ctx: &mut TuiPaintContext) {
        let model = self.model.lock();
        let colors = model.colors();
        render_grid_handler(model.alt_screen().grid_handler(), area, buffer, &colors);
    }

    fn cursor_position(&self, area: TuiRect, _ctx: &mut TuiPaintContext) -> Option<(u16, u16)> {
        let model = self.model.lock();
        let alt = model.alt_screen();
        if !alt.is_mode_set(TermMode::SHOW_CURSOR) {
            return None;
        }
        let grid = alt.grid_handler();
        let point = grid.cursor_render_point();
        // The alt screen has no scrollback, but subtract history defensively so
        // the cursor maps to a visible (screen-relative) row.
        let row = point.row.checked_sub(grid.history_size())?;
        let col = u16::try_from(point.col).ok()?;
        let row = u16::try_from(row).ok()?;
        if col >= area.width || row >= area.height {
            return None;
        }
        Some((area.x.saturating_add(col), area.y.saturating_add(row)))
    }

    fn dispatch_event(
        &mut self,
        event: &TuiEvent,
        area: TuiRect,
        event_ctx: &mut TuiEventContext,
        _ctx: &mut TuiLayoutContext,
        _app: &AppContext,
    ) -> bool {
        let bytes = {
            let model = self.model.lock();
            match event {
                TuiEvent::KeyDown {
                    keystroke,
                    chars,
                    details,
                    is_composing: false,
                } => KeystrokeWithDetails {
                    keystroke,
                    key_without_modifiers: details.key_without_modifiers.as_deref(),
                    chars: Some(chars.as_str()),
                }
                .to_pty_bytes(model.deref()),
                TuiEvent::KeyDown {
                    is_composing: true, ..
                } => None,
                _ => mouse_event_to_pty_bytes(
                    event,
                    area,
                    |mode| model.is_term_mode_set(mode),
                    model.deref(),
                ),
            }
        };
        let Some(bytes) = bytes else {
            return false;
        };
        event_ctx.dispatch_typed_action(TuiTerminalSessionAction::ForwardToPty(bytes));
        true
    }
}

#[cfg(test)]
#[path = "alt_screen_view_tests.rs"]
mod tests;
