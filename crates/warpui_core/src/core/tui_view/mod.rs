//! The TUI-side view abstraction, mirroring the GUI [`View`](crate::View) layer
//! but rendering to an abstract, backend-owned output so that `warpui_core` never
//! depends on the (downstream) `warpui_tui` crate.
//!
//! All items in this module are gated behind the `tui` feature. They reuse the
//! shared application core ([`AppContextImpl`](crate::core::AppContextImpl) and
//! the model/async/action/subscription machinery) exactly as the GUI view layer
//! does.

mod context;
mod handle;

use std::any::Any;

pub use self::context::*;
pub use self::handle::*;
use super::EntityId;
use crate::core::tui_backend::TuiBackend;
use crate::core::{AppContextImpl, BlurContext, CursorInfo, FocusContext};
use crate::{keymap, AppContext, Entity, WindowId};

/// The TUI analogue of [`View`](crate::View).
///
/// A `TuiView` holds instance state and can render itself to its own
/// [`RenderOutput`](TuiView::RenderOutput). Unlike the GUI [`View`](crate::View),
/// the render output type is owned by the view (and ultimately the `warpui_tui`
/// crate that defines concrete TUI elements), not by `warpui_core`; the shared
/// core only ever sees it type-erased as [`TuiBackend::RenderOutput`].
pub trait TuiView: Entity {
    /// What this view renders to. The concrete element/buffer type lives in the
    /// downstream `warpui_tui` crate; the shared core only stores it erased.
    type RenderOutput: 'static;

    /// Returns a unique name for this implementation of `TuiView`.
    fn ui_name() -> &'static str;

    /// Produces this view's render output.
    fn render_tui(&self, ctx: &AppContextImpl<TuiBackend>) -> Self::RenderOutput;

    /// Handles the view or its descendent receiving focus.
    fn on_focus(&mut self, _focus_ctx: &FocusContext, _ctx: &mut TuiViewContext<Self>) {}

    /// Handles the view or its descendent losing focus.
    fn on_blur(&mut self, _blur_ctx: &BlurContext, _ctx: &mut TuiViewContext<Self>) {}

    /// Reports the active cursor position for the view, if any.
    fn active_cursor_position(&self, _ctx: &TuiViewContext<Self>) -> Option<CursorInfo> {
        None
    }

    /// Handles the view's containing window closing.
    fn on_window_closed(&mut self, _ctx: &mut TuiViewContext<Self>) {}

    /// Called when the view is transferred from one window to another.
    fn on_window_transferred(
        &mut self,
        _source_window_id: WindowId,
        _target_window_id: WindowId,
        _ctx: &mut TuiViewContext<Self>,
    ) {
    }

    /// Returns a representation of the current UI context for use in computing
    /// the set of valid actions/keyboard shortcuts.
    fn keymap_context(&self, _: &AppContext) -> keymap::Context {
        Self::default_keymap_context()
    }

    /// Returns the default context for a view.
    fn default_keymap_context() -> keymap::Context {
        let mut ctx = keymap::Context::default();
        ctx.set.insert(Self::ui_name());
        ctx
    }

    /// Allows a view to hook into any interactions with it or its children.
    fn self_or_child_interacted_with(&self, _ctx: &mut TuiViewContext<Self>) {}
}

/// The TUI analogue of [`TypedActionView`](crate::TypedActionView): a `TuiView`
/// that handles typed actions dispatched through the shared core.
pub trait TuiTypedActionView: TuiView {
    type Action: crate::Action;

    /// Handles an action of type [`Self::Action`](TuiTypedActionView::Action)
    /// dispatched from this view or a descendant.
    fn handle_action(&mut self, _action: &Self::Action, _ctx: &mut TuiViewContext<Self>) {}
}

/// The object-safe surface stored per window for the TUI backend, mirroring
/// [`AnyView`](crate::AnyView). Carries the render + focus/keymap hooks the
/// shared core needs without naming the concrete view type.
///
/// This is the TUI value of [`TuiBackend::AnyView`].
pub trait AnyTuiView {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn ui_name(&self) -> &'static str;
    fn render_tui(&self, app: &AppContextImpl<TuiBackend>) -> Box<dyn Any>;
    fn on_focus(
        &mut self,
        focus_ctx: &FocusContext,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    );
    fn on_blur(
        &mut self,
        blur_ctx: &BlurContext,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    );
    fn keymap_context(&self, app: &AppContext) -> keymap::Context;
    fn active_cursor_position(
        &self,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    ) -> Option<CursorInfo>;
    fn on_window_closed(&mut self, app: &mut AppContext, window_id: WindowId, view_id: EntityId);
    fn on_window_transferred(
        &mut self,
        source_window_id: WindowId,
        target_window_id: WindowId,
        app: &mut AppContext,
        view_id: EntityId,
    );
    fn self_or_child_interacted_with(
        &self,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    );
}

impl<T> AnyTuiView for T
where
    T: TuiView,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn ui_name(&self) -> &'static str {
        T::ui_name()
    }

    fn render_tui(&self, app: &AppContextImpl<TuiBackend>) -> Box<dyn Any> {
        Box::new(TuiView::render_tui(self, app))
    }

    fn on_focus(
        &mut self,
        focus_ctx: &FocusContext,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    ) {
        let mut ctx = TuiViewContext::new(app, window_id, view_id);
        TuiView::on_focus(self, focus_ctx, &mut ctx);
    }

    fn on_blur(
        &mut self,
        blur_ctx: &BlurContext,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    ) {
        let mut ctx = TuiViewContext::new(app, window_id, view_id);
        TuiView::on_blur(self, blur_ctx, &mut ctx);
    }

    fn keymap_context(&self, app: &AppContext) -> keymap::Context {
        TuiView::keymap_context(self, app)
    }

    fn active_cursor_position(
        &self,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    ) -> Option<CursorInfo> {
        let ctx = TuiViewContext::new(app, window_id, view_id);
        TuiView::active_cursor_position(self, &ctx)
    }

    fn on_window_closed(&mut self, app: &mut AppContext, window_id: WindowId, view_id: EntityId) {
        let mut ctx = TuiViewContext::new(app, window_id, view_id);
        TuiView::on_window_closed(self, &mut ctx);
    }

    fn on_window_transferred(
        &mut self,
        source_window_id: WindowId,
        target_window_id: WindowId,
        app: &mut AppContext,
        view_id: EntityId,
    ) {
        let mut ctx = TuiViewContext::new(app, target_window_id, view_id);
        TuiView::on_window_transferred(self, source_window_id, target_window_id, &mut ctx);
    }

    fn self_or_child_interacted_with(
        &self,
        app: &mut AppContext,
        window_id: WindowId,
        view_id: EntityId,
    ) {
        let mut ctx = TuiViewContext::new(app, window_id, view_id);
        TuiView::self_or_child_interacted_with(self, &mut ctx);
    }
}

#[cfg(test)]
#[path = "tui_view_tests.rs"]
mod tests;
