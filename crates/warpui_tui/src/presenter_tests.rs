//! Headless tests for [`TuiPresenter`]. They drive layout + paint against local
//! [`TuiElement`] test-doubles (so they do not depend on task 3.2's concrete
//! elements) and assert the composited [`TuiBuffer`] via `to_lines` plus the
//! surfaced cursor. The child-view test registers a real [`TuiView`] in a test
//! [`App`] and resolves it through the app, exactly as the live elements will.

use warpui_core::platform::WindowStyle;
use warpui_core::{
    AddWindowOptions, App, AppContext, Entity, EntityId, TuiTypedActionView, TuiView,
};

use super::TuiPresenter;
use crate::elements::TuiPresentationContext;
use crate::{
    Cell, TuiBuffer, TuiConstraint, TuiElement, TuiRect, TuiRenderOutput, TuiSize, TuiStyle,
};

// --- Test-double elements -------------------------------------------------

/// A single line of text: as wide as its content, one row tall.
struct TextDouble {
    text: String,
}

impl TextDouble {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }

    fn width(&self) -> u16 {
        self.text.chars().count() as u16
    }
}

impl TuiElement for TextDouble {
    fn layout(&mut self, constraint: TuiConstraint) -> TuiSize {
        constraint.clamp(TuiSize::new(self.width(), 1))
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer) {
        buffer.set_str(area.x, area.y, area.width, &self.text, TuiStyle::default());
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// A vertical stack: each child is laid out at the column's width and stacked
/// top-to-bottom. Records per-child sizes at layout time so `render`/
/// `cursor_position` (which take `&self`) can place children consistently.
struct ColumnDouble {
    children: Vec<Box<dyn TuiElement>>,
    child_sizes: Vec<TuiSize>,
}

impl ColumnDouble {
    fn new(children: Vec<Box<dyn TuiElement>>) -> Self {
        Self {
            children,
            child_sizes: Vec::new(),
        }
    }
}

impl TuiElement for ColumnDouble {
    fn layout(&mut self, constraint: TuiConstraint) -> TuiSize {
        self.child_sizes.clear();
        let mut total_height = 0u16;
        let mut max_width = 0u16;
        for child in &mut self.children {
            let available = TuiSize::new(
                constraint.max.width,
                constraint.max.height.saturating_sub(total_height),
            );
            let size = child.layout(TuiConstraint::loose(available));
            total_height = total_height.saturating_add(size.height);
            max_width = max_width.max(size.width);
            self.child_sizes.push(size);
        }
        constraint.clamp(TuiSize::new(max_width, total_height))
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer) {
        let mut remaining = area;
        for (child, size) in self.children.iter().zip(&self.child_sizes) {
            let (row, rest) = remaining.split_top(size.height);
            let child_area = TuiRect::new(row.x, row.y, size.width.min(row.width), row.height);
            child.render(child_area, buffer);
            remaining = rest;
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children
            .iter()
            .map(|child| child.desired_height(width))
            .sum()
    }

    fn present(&mut self, ctx: &mut TuiPresentationContext<'_>) {
        for child in &mut self.children {
            child.present(ctx);
        }
    }

    fn cursor_position(&self, area: TuiRect) -> Option<(u16, u16)> {
        let mut remaining = area;
        for (child, size) in self.children.iter().zip(&self.child_sizes) {
            let (row, rest) = remaining.split_top(size.height);
            let child_area = TuiRect::new(row.x, row.y, size.width.min(row.width), row.height);
            if let Some((cx, cy)) = child.cursor_position(child_area) {
                return Some((child_area.x - area.x + cx, child_area.y - area.y + cy));
            }
            remaining = rest;
        }
        None
    }
}

/// A styled container that fills its area with `fill` then paints its child
/// inset by `padding` on every side.
struct ContainerDouble {
    child: Box<dyn TuiElement>,
    padding: u16,
    fill: char,
    child_size: TuiSize,
}

impl ContainerDouble {
    fn new(child: Box<dyn TuiElement>, padding: u16, fill: char) -> Self {
        Self {
            child,
            padding,
            fill,
            child_size: TuiSize::ZERO,
        }
    }
}

impl TuiElement for ContainerDouble {
    fn layout(&mut self, constraint: TuiConstraint) -> TuiSize {
        let inset = self.padding.saturating_mul(2);
        let inner_max = TuiSize::new(
            constraint.max.width.saturating_sub(inset),
            constraint.max.height.saturating_sub(inset),
        );
        let size = self.child.layout(TuiConstraint::loose(inner_max));
        self.child_size = size;
        constraint.clamp(TuiSize::new(
            size.width.saturating_add(inset),
            size.height.saturating_add(inset),
        ))
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer) {
        buffer.fill(area, Cell::new(self.fill.to_string(), TuiStyle::default()));
        let inner = area.inset(self.padding);
        let child_area = TuiRect::new(
            inner.x,
            inner.y,
            self.child_size.width.min(inner.width),
            self.child_size.height.min(inner.height),
        );
        self.child.render(child_area, buffer);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let inset = self.padding.saturating_mul(2);
        self.child
            .desired_height(width.saturating_sub(inset))
            .saturating_add(inset)
    }

    fn present(&mut self, ctx: &mut TuiPresentationContext<'_>) {
        self.child.present(ctx);
    }
}

/// A leaf that owns the cursor, reporting it at a fixed offset within its area.
struct CursorDouble {
    offset: (u16, u16),
}

impl CursorDouble {
    fn new(offset: (u16, u16)) -> Self {
        Self { offset }
    }
}

impl TuiElement for CursorDouble {
    fn layout(&mut self, constraint: TuiConstraint) -> TuiSize {
        constraint.clamp(TuiSize::new(5, 1))
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer) {
        buffer.set_str(area.x, area.y, area.width, "INPUT", TuiStyle::default());
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }

    fn cursor_position(&self, _area: TuiRect) -> Option<(u16, u16)> {
        Some(self.offset)
    }
}

/// A [`TuiChildView`]-style element: it embeds an already-rendered child view's
/// element tree and records the embedded view as a child during the present
/// pass. Delegates layout/paint/cursor to the embedded tree.
struct ChildViewElement {
    child_view_id: EntityId,
    inner: Box<dyn TuiElement>,
}

impl TuiElement for ChildViewElement {
    fn layout(&mut self, constraint: TuiConstraint) -> TuiSize {
        self.inner.layout(constraint)
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer) {
        self.inner.render(area, buffer);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.inner.desired_height(width)
    }

    fn present(&mut self, ctx: &mut TuiPresentationContext<'_>) {
        ctx.enter_child(self.child_view_id);
        self.inner.present(ctx);
        ctx.exit_child();
    }

    fn cursor_position(&self, area: TuiRect) -> Option<(u16, u16)> {
        self.inner.cursor_position(area)
    }
}

// --- Real views for the child-view recursion test -------------------------

fn window_options() -> AddWindowOptions {
    AddWindowOptions {
        window_style: WindowStyle::NotStealFocus,
        ..Default::default()
    }
}

/// Minimal window root; never presented directly.
#[derive(Default)]
struct RootStub;

impl Entity for RootStub {
    type Event = ();
}

impl TuiView for RootStub {
    type RenderOutput = ();
    fn ui_name() -> &'static str {
        "RootStub"
    }
    fn render_tui(&self, _ctx: &AppContext) {}
}

impl TuiTypedActionView for RootStub {
    type Action = ();
}

/// A registered child view whose output is a single line of text.
struct LeafView;

impl Entity for LeafView {
    type Event = ();
}

impl TuiView for LeafView {
    type RenderOutput = TuiRenderOutput;
    fn ui_name() -> &'static str {
        "LeafView"
    }
    fn render_tui(&self, _ctx: &AppContext) -> TuiRenderOutput {
        Box::new(TextDouble::new("CHILD"))
    }
}

/// A parent view: a header above an embedded child view, which it resolves
/// through the app at render time.
struct ParentView {
    child_view_id: EntityId,
}

impl Entity for ParentView {
    type Event = ();
}

impl TuiView for ParentView {
    type RenderOutput = TuiRenderOutput;
    fn ui_name() -> &'static str {
        "ParentView"
    }
    fn render_tui(&self, ctx: &AppContext) -> TuiRenderOutput {
        let child_tree = ctx
            .render_tui_view(self.child_view_id)
            .and_then(|output| output.downcast::<Box<dyn TuiElement>>().ok())
            .map(|boxed| *boxed)
            .expect("child view should resolve to a boxed element");
        let child = ChildViewElement {
            child_view_id: self.child_view_id,
            inner: child_tree,
        };
        Box::new(ColumnDouble::new(vec![
            Box::new(TextDouble::new("HEADER")),
            Box::new(child),
        ]))
    }
}

// --- Tests ----------------------------------------------------------------

#[test]
fn paints_single_root_element_into_area() {
    let mut presenter = TuiPresenter::new();
    let frame = presenter.present_element(
        Box::new(TextDouble::new("HELLO")),
        TuiRect::new(0, 0, 10, 1),
    );
    assert_eq!(frame.buffer.to_lines(), vec!["HELLO     "]);
    assert_eq!(frame.cursor, None);
}

#[test]
fn composites_nested_container_column_text_with_offsets() {
    let column = ColumnDouble::new(vec![
        Box::new(TextDouble::new("AB")),
        Box::new(TextDouble::new("CDE")),
    ]);
    let container = ContainerDouble::new(Box::new(column), 1, '.');

    let mut presenter = TuiPresenter::new();
    let frame = presenter.present_element(Box::new(container), TuiRect::new(0, 0, 5, 4));

    assert_eq!(
        frame.buffer.to_lines(),
        vec![".....", ".AB..", ".CDE.", "....."],
    );
}

#[test]
fn surfaces_cursor_at_absolute_coordinates() {
    let column = ColumnDouble::new(vec![
        Box::new(TextDouble::new("HEADER")),
        Box::new(CursorDouble::new((2, 0))),
    ]);

    let mut presenter = TuiPresenter::new();
    let frame = presenter.present_element(Box::new(column), TuiRect::new(0, 0, 8, 2));

    // The cursor element sits on row 1 (below the header) at column 2.
    assert_eq!(frame.cursor, Some((2, 1)));
}

#[test]
fn recurses_into_registered_child_view() {
    App::test((), |mut app| async move {
        let (window_id, _root) =
            app.update(|ctx| ctx.add_tui_window(window_options(), |_| RootStub));

        let child = app.update(|ctx| ctx.add_tui_view(window_id, |_| LeafView));
        let child_view_id = child.id();
        let parent =
            app.update(|ctx| ctx.add_tui_view(window_id, move |_| ParentView { child_view_id }));

        let mut presenter = TuiPresenter::new();
        let frame = app.read(|ctx| presenter.present(ctx, &parent, TuiRect::new(0, 0, 8, 3)));

        // The child view's output is painted directly below the header, at the
        // area the layout allocated to the embedded child-view element.
        assert_eq!(
            frame.buffer.to_lines(),
            vec!["HEADER  ", "CHILD   ", "        "],
        );

        // The presentation pass recorded the embedded view as a child of the root.
        assert_eq!(presenter.parent_view(child_view_id), Some(parent.id()));
    });
}
