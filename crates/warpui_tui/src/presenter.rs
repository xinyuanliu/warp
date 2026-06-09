//! The TUI presenter: turns a root [`TuiView`]'s render output into a painted
//! [`TuiFrame`] (a cell [`TuiBuffer`] plus the absolute cursor cell).
//!
//! # Layout pass ordering: measure → arrange → paint
//!
//! 1. **measure** — the root element is measured against a loose
//!    [`TuiConstraint`] bounded by the target area, returning the size it wants
//!    (within that box).
//! 2. **arrange** — that size is anchored at the area's origin, producing the
//!    absolute rectangle the root occupies. Container elements recurse this
//!    measure/arrange internally for their children (the presenter only drives
//!    the root; the element tree composes itself).
//! 3. **paint** — the root paints into its arranged rectangle of a fresh buffer.
//!    Each container paints its children into their sub-rectangles, so the whole
//!    tree composites into one buffer.
//!
//! After arranging, the presenter walks the tree once more via
//! [`TuiElement::present`] to record parent/child *view* relationships (for a
//! [`TuiChildView`]-style element that embeds a sub-view); this ancestry is what
//! lets the runtime attribute events and actions to the right view.
//!
//! # Child views
//!
//! A child view is a full [`TuiView`] registered in the app. It is embedded by
//! resolving it through the app — [`AppContext::render_tui_view`] renders it to
//! its boxed element tree — and wrapping that output in a child-view element
//! during the *parent* view's render (which has app access). The presenter then
//! lays out and paints the composed tree, so the child's output lands at exactly
//! the area the layout allocated to it, and the [`present`](TuiElement::present)
//! pass records the embedded view as a child of its parent.

use std::collections::HashMap;

use warpui_core::{AppContext, EntityId, TuiView, TuiViewHandle};

use crate::elements::TuiPresentationContext;
use crate::{TuiBuffer, TuiConstraint, TuiElement, TuiRect, TuiSize};

/// A painted frame: the composited cell [`TuiBuffer`] plus the absolute cursor
/// position (in buffer cell coordinates), if a focused element owns the cursor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiFrame {
    /// The composited cell grid, covering the columns/rows up to the painted
    /// area's right/bottom edge.
    pub buffer: TuiBuffer,
    /// The absolute `(x, y)` cell the terminal cursor should occupy, if any.
    pub cursor: Option<(u16, u16)>,
}

impl TuiFrame {
    /// A blank frame sized to cover `area`, with no cursor.
    fn blank(area: TuiRect) -> Self {
        Self {
            buffer: TuiBuffer::new(buffer_size_for(area)),
            cursor: None,
        }
    }
}

/// Lays out and paints a [`TuiView`]'s element tree into a [`TuiFrame`].
///
/// The presenter retains the parent/child *view* ancestry recorded during the
/// last [`present`](Self::present) so the runtime can resolve which view an
/// event-handling element belongs to.
#[derive(Default)]
pub struct TuiPresenter {
    /// Maps each embedded child view to its parent view, as recorded by the most
    /// recent presentation pass.
    parent_by_child: HashMap<EntityId, EntityId>,
}

impl TuiPresenter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Renders the root view through the app, then lays it out and paints it into
    /// `area`, returning the composited [`TuiFrame`].
    ///
    /// The root is resolved via [`AppContext::render_tui_view`] and downcast back
    /// to the concrete boxed [`TuiElement`]; a view that does not render to a
    /// [`TuiRenderOutput`](crate::TuiRenderOutput) yields a blank frame.
    pub fn present<V: TuiView>(
        &mut self,
        app: &AppContext,
        root: &TuiViewHandle<V>,
        area: TuiRect,
    ) -> TuiFrame {
        let root_view_id = root.id();
        let Some(element) = app
            .render_tui_view(root_view_id)
            .and_then(|output| output.downcast::<Box<dyn TuiElement>>().ok())
            .map(|boxed| *boxed)
        else {
            return TuiFrame::blank(area);
        };
        self.paint_tree(Some(root_view_id), element, area)
    }

    /// Lays out and paints an already-rendered element tree into `area`.
    ///
    /// This is the backend-agnostic core of [`present`](Self::present), exposed so
    /// the runtime (and tests) can drive layout/paint for an element tree that was
    /// produced outside the app's view registry. No view-ancestry is recorded.
    pub fn present_element(&mut self, root: Box<dyn TuiElement>, area: TuiRect) -> TuiFrame {
        self.paint_tree(None, root, area)
    }

    /// Returns the parent view of `child_view_id` as recorded by the last
    /// presentation pass, if it was embedded as a child view.
    pub fn parent_view(&self, child_view_id: EntityId) -> Option<EntityId> {
        self.parent_by_child.get(&child_view_id).copied()
    }

    fn paint_tree(
        &mut self,
        root_view_id: Option<EntityId>,
        mut root: Box<dyn TuiElement>,
        area: TuiRect,
    ) -> TuiFrame {
        // measure: ask the root how big it wants to be within the area.
        let measured = root.layout(TuiConstraint::loose(area.size()));

        // arrange: anchor the measured size at the area's origin (the size is
        // already within the area, but clamp defensively so writes stay in bounds).
        let arranged = TuiRect::new(
            area.x,
            area.y,
            measured.width.min(area.width),
            measured.height.min(area.height),
        );

        // record parent/child view ancestry for the (root) view tree.
        self.parent_by_child.clear();
        if let Some(root_view_id) = root_view_id {
            let mut ctx = TuiPresentationContext::new(root_view_id, &mut self.parent_by_child);
            root.present(&mut ctx);
        }

        // paint: composite the whole tree into a fresh buffer.
        let mut buffer = TuiBuffer::new(buffer_size_for(area));
        root.render(arranged, &mut buffer);

        // cursor: lift the root-relative cursor offset to absolute coordinates.
        let cursor = root
            .cursor_position(arranged)
            .map(|(x, y)| (arranged.x.saturating_add(x), arranged.y.saturating_add(y)));

        TuiFrame { buffer, cursor }
    }
}

/// The buffer size needed to hold everything painted within `area`: it spans
/// from the origin to the area's right/bottom edge, so absolute coordinates
/// (including any area offset) index correctly.
fn buffer_size_for(area: TuiRect) -> TuiSize {
    TuiSize::new(area.right(), area.bottom())
}

#[cfg(test)]
#[path = "presenter_tests.rs"]
mod tests;
