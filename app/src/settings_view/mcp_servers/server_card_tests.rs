use pathfinder_geometry::vector::vec2f;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{CornerRadius, Element, Fill, Radius};
use warpui::platform::WindowStyle;
use warpui::ui_components::chip::Chip;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    App, AppContext, Entity, EntityIdSet, Presenter, SingletonEntity, TypedActionView, View,
    WindowInvalidation,
};

use crate::appearance::Appearance;
use crate::test_util::terminal::initialize_app_for_terminal_view;

/// Renders a single tool-style [`Chip`], optionally with the trailing right
/// margin the MCP tool chips use for inter-chip spacing.
struct ChipTestView {
    with_margin: bool,
}

impl Entity for ChipTestView {
    type Event = ();
}

impl TypedActionView for ChipTestView {
    type Action = ();
}

impl View for ChipTestView {
    fn ui_name() -> &'static str {
        "ChipTestView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let margin = self.with_margin.then_some(Coords {
            top: 0.,
            bottom: 0.,
            left: 0.,
            right: 6.,
        });

        Chip::new(
            "tool_name".to_string(),
            UiComponentStyles {
                margin,
                font_family_id: Some(appearance.ui_font_family()),
                font_size: Some(12.),
                background: Some(internal_colors::neutral_4(appearance.theme()).into()),
                border_radius: Some(CornerRadius::with_all(Radius::Pixels(5.))),
                ..Default::default()
            },
        )
        .build()
        .finish()
    }
}

/// Renders a `ChipTestView` and returns the width of the chip's painted
/// background rect (the pill). The chip is the only element in the view, so it
/// paints exactly one rect.
fn chip_pill_width(app: &mut App, with_margin: bool) -> f32 {
    let (window_id, _view) = app.add_window(WindowStyle::NotStealFocus, move |_| ChipTestView {
        with_margin,
    });
    let mut presenter = Presenter::new(window_id);
    let mut updated = EntityIdSet::default();
    updated.insert(app.root_view_id(window_id).expect("root view should exist"));
    let invalidation = WindowInvalidation {
        updated,
        ..Default::default()
    };

    app.update(move |ctx| {
        presenter.invalidate(invalidation, ctx);
        let scene = presenter.build_scene(vec2f(400., 200.), 1., None, ctx);
        // Transparent containers (the label wrappers) also paint rects, so
        // select the chip's filled pill by its non-`None` background.
        let widths: Vec<f32> = scene
            .layers()
            .flat_map(|layer| layer.rects.iter())
            .filter(|rect| !matches!(rect.background, Fill::None))
            .map(|rect| rect.bounds.width())
            .collect();
        assert_eq!(
            widths.len(),
            1,
            "chip should paint exactly one filled pill rect, got {widths:?}"
        );
        widths[0]
    })
}

/// A chip's `margin` is meant to space it from its siblings and must be applied
/// only to the outer chip container, never leak into the inner label. This
/// guards the MCP servers "tools available" chips, whose trailing right margin
/// used to also be applied to the label and show up as extra padding on the
/// right side of the tool text (the pill grew wider on the right).
#[test]
fn chip_right_margin_does_not_widen_pill() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let with_margin = chip_pill_width(&mut app, true);
        let without_margin = chip_pill_width(&mut app, false);

        assert!(
            (with_margin - without_margin).abs() < 0.5,
            "the chip's right margin should not add padding inside the pill: \
             with_margin width = {with_margin}, without_margin width = {without_margin}"
        );
    });
}
