//! The TUI [`Backend`] instantiation, gated behind the `tui` feature.
//!
//! `TuiBackend` mirrors [`GuiBackend`](super::GuiBackend) but keeps its render
//! output abstract (`Box<dyn Any>`) so `warpui_core` never names a concrete TUI
//! element type and therefore never depends on the downstream `warpui_tui`
//! crate. The concrete element/buffer types are defined in `warpui_tui` (a later
//! milestone) and recovered by downcasting this erased output.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::backend::{Backend, BackendView, ErasedView};
use super::tui_view::{AnyTuiView, TuiView};
use super::AppContextImpl;
use crate::presenter::PositionCache;
use crate::{Presenter, WindowId};

/// The TUI backend marker.
pub struct TuiBackend;

impl Backend for TuiBackend {
    /// Abstract: the concrete TUI element/buffer type lives in `warpui_tui` and
    /// is recovered by downcasting. Keeping this `Box<dyn Any>` is what prevents
    /// a `warpui_core -> warpui_tui` dependency cycle.
    type RenderOutput = Box<dyn Any>;

    type AnyView = dyn AnyTuiView;

    /// The TUI backend's own presentation state. The real TUI render/event loop
    /// (layout + paint into a cell buffer) lives downstream in `warpui_tui`'s
    /// `TuiPresenter`; this is the core-side handle for it.
    type Presenter = TuiPresenterState;
}

/// Presentation state for the TUI backend, stored on `AppContextImpl<TuiBackend>`
/// as `B::Presenter`. This is a first-class, TUI-owned type (replacing the M2
/// placeholder that reused [`GuiPresenterState`](super::GuiPresenterState)); the
/// backend-agnostic window-invalidation bookkeeping now lives on
/// `AppContextImpl<B>` directly, so it is not duplicated here.
///
/// NOTE (deferred cleanup): the GUI presenter map + position cache below are a
/// residual compile bridge. The shared scene/event/focus plumbing in `app.rs`
/// (`presenter`, `build_scene`, `handle_window_event`, focus/blur, …) is not yet
/// cfg-split between backends and still references these fields even on a TUI
/// build, where they go unused (the GUI presenter is inert). Fully shedding them
/// requires cfg-gating that GUI scene path and is intentionally left out of this
/// task to keep the GUI + `--features tui` gates stable.
#[derive(Default)]
pub struct TuiPresenterState {
    pub(crate) presenters: HashMap<WindowId, Rc<RefCell<Presenter>>>,
    pub(crate) last_frame_position_cache: HashMap<WindowId, PositionCache>,
}

impl ErasedView<TuiBackend> for dyn AnyTuiView {
    fn render(&self, app: &AppContextImpl<TuiBackend>) -> Box<dyn Any> {
        AnyTuiView::render_tui(self, app)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        AnyTuiView::as_any_mut(self)
    }
}

impl<T: TuiView> BackendView<TuiBackend> for T {
    fn into_any_view(self: Box<Self>) -> Box<dyn AnyTuiView> {
        self
    }
}
