//! The TUI [`Backend`] instantiation, gated behind the `tui` feature.
//!
//! `TuiBackend` mirrors [`GuiBackend`](super::GuiBackend) but keeps its render
//! output abstract (`Box<dyn Any>`) so `warpui_core` never names a concrete TUI
//! element type and therefore never depends on the downstream `warpui_tui`
//! crate. The concrete element/buffer types are defined in `warpui_tui` (a later
//! milestone) and recovered by downcasting this erased output.

use std::any::Any;

use super::backend::{Backend, BackendView, ErasedView, GuiPresenterState};
use super::tui_view::{AnyTuiView, TuiView};
use super::AppContextImpl;

/// The TUI backend marker.
pub struct TuiBackend;

impl Backend for TuiBackend {
    /// Abstract: the concrete TUI element/buffer type lives in `warpui_tui` and
    /// is recovered by downcasting. Keeping this `Box<dyn Any>` is what prevents
    /// a `warpui_core -> warpui_tui` dependency cycle.
    type RenderOutput = Box<dyn Any>;

    type AnyView = dyn AnyTuiView;

    /// Reuses [`GuiPresenterState`] so the shared core retains the
    /// window-invalidation bookkeeping that `notify`/dirty tracking depend on.
    /// The real TUI presentation layer arrives in a later milestone; until then
    /// the GUI presenter state simply goes unused on a TUI build.
    type Presenter = GuiPresenterState;
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
