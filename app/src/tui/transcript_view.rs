//! [`TuiTranscriptView`]: stores plain-text prompt entries (non-`!` submissions)
//! for display. Command blocks are rendered directly from the `TerminalModel`'s
//! block list by `TuiBlockListElement` in `tui.rs`; this view only holds the
//! local text entries that don't correspond to model blocks.

use warpui_core::elements::tui::{
    Color, Modifier, TuiBuffer, TuiColumn, TuiConstraint, TuiElement, TuiRect, TuiSize, TuiStyle,
    TuiText,
};
use warpui_core::{AppContext, Entity, TuiView, ViewContext};

/// Near-white entry text (`#f1f1f1`), bold like the user prompt in the mock.
const ENTRY_COLOR: Color = Color::Rgb(0xf1, 0xf1, 0xf1);

/// A single transcript entry: a submitted plain-text prompt.
enum TranscriptEntry {
    Prompt(String),
}

impl TranscriptEntry {
    fn render(&self) -> Box<dyn TuiElement> {
        let style = TuiStyle::default()
            .fg(ENTRY_COLOR)
            .add_modifier(Modifier::BOLD);
        match self {
            TranscriptEntry::Prompt(text) => Box::new(TuiText::new(text.clone()).with_style(style)),
        }
    }
}

#[derive(Default)]
pub struct TuiTranscriptView {
    entries: Vec<TranscriptEntry>,
}

impl TuiTranscriptView {
    /// Appends `text` as the newest transcript entry and schedules a redraw.
    pub fn append(&mut self, text: String, ctx: &mut ViewContext<Self>) {
        self.entries.push(TranscriptEntry::Prompt(text));
        ctx.notify();
    }
}

impl Entity for TuiTranscriptView {
    type Event = ();
}

impl TuiView for TuiTranscriptView {
    fn ui_name() -> &'static str {
        "TuiTranscriptView"
    }

    fn render(&self, _ctx: &AppContext) -> Box<dyn TuiElement> {
        if self.entries.is_empty() {
            return Box::new(TuiColumn::new());
        }

        let children: Vec<Box<dyn TuiElement>> = self
            .entries
            .iter()
            .flat_map(|entry| {
                [
                    entry.render(),
                    Box::new(TuiText::new(" ")) as Box<dyn TuiElement>,
                ]
            })
            .collect();

        Box::new(BottomAnchoredColumn::new(children))
    }
}

/// A vertical stack that anchors its children to the bottom of the area it is
/// given: when the content is shorter than the area it sits flush against the
/// bottom edge, and when it is taller the top rows are clipped (so the newest,
/// bottom-most content stays visible).
pub(crate) struct BottomAnchoredColumn {
    children: Vec<Box<dyn TuiElement>>,
}

impl BottomAnchoredColumn {
    pub(crate) fn new(children: Vec<Box<dyn TuiElement>>) -> Self {
        Self { children }
    }

    fn child_heights(&self, width: u16) -> Vec<u16> {
        self.children
            .iter()
            .map(|child| child.desired_height(width))
            .collect()
    }
}

impl TuiElement for BottomAnchoredColumn {
    fn layout(&mut self, constraint: TuiConstraint) -> TuiSize {
        constraint.clamp(constraint.max)
    }

    fn render(&self, area: TuiRect, buffer: &mut TuiBuffer) {
        if area.is_empty() {
            return;
        }
        let width = area.width;
        let heights = self.child_heights(width);
        let total = heights.iter().fold(0u16, |acc, &h| acc.saturating_add(h));
        if total == 0 {
            return;
        }

        let mut scratch = TuiBuffer::empty(TuiRect::new(0, 0, width, total));
        let mut y = 0u16;
        for (child, &height) in self.children.iter().zip(&heights) {
            if height == 0 {
                continue;
            }
            child.render(TuiRect::new(0, y, width, height), &mut scratch);
            y = y.saturating_add(height);
        }

        let visible = total.min(area.height);
        let src_top = total - visible;
        let dst_top = area.y + (area.height - visible);
        for row in 0..visible {
            for col in 0..width {
                let cell = scratch[(col, src_top + row)].clone();
                if let Some(dst) = buffer.cell_mut((area.x + col, dst_top + row)) {
                    *dst = cell;
                }
            }
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.child_heights(width)
            .iter()
            .fold(0u16, |acc, &h| acc.saturating_add(h))
    }
}

#[cfg(test)]
#[path = "transcript_view_tests.rs"]
mod tests;
