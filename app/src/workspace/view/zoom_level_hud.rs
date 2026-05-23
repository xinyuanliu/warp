use std::time::Duration;

use warpui::elements::{ConstrainedBox, Container, CornerRadius, DropShadow, Empty, Radius, Text};
use warpui::fonts::{Properties, Weight};
use warpui::r#async::{SpawnedFutureHandle, Timer};
use warpui::{AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use crate::appearance::Appearance;
use crate::ui_components::blended_colors;

const DISMISS_AFTER: Duration = Duration::from_secs(1);
const HUD_MIN_WIDTH: f32 = 56.;
const HUD_FONT_SIZE: f32 = 13.;

pub struct ZoomLevelHud {
    visible_zoom_level: Option<u16>,
    dismiss_handle: Option<SpawnedFutureHandle>,
}

impl ZoomLevelHud {
    pub fn new(_: &mut ViewContext<Self>) -> Self {
        Self {
            visible_zoom_level: None,
            dismiss_handle: None,
        }
    }

    pub fn show_zoom_level(&mut self, zoom_level: u16, ctx: &mut ViewContext<Self>) {
        if let Some(handle) = self.dismiss_handle.take() {
            handle.abort();
        }

        self.visible_zoom_level = Some(zoom_level);
        self.dismiss_handle = Some(ctx.spawn_abortable(
            Timer::after(DISMISS_AFTER),
            |view, _, ctx| view.dismiss(ctx),
            |_, _| {},
        ));
        ctx.notify();
    }

    pub fn dismiss(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(handle) = self.dismiss_handle.take() {
            handle.abort();
        }

        if self.visible_zoom_level.take().is_some() {
            ctx.notify();
        }
    }

    #[cfg(test)]
    pub fn visible_zoom_level(&self) -> Option<u16> {
        self.visible_zoom_level
    }
}

impl View for ZoomLevelHud {
    fn ui_name() -> &'static str {
        "ZoomLevelHud"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let Some(zoom_level) = self.visible_zoom_level else {
            return Empty::new().finish();
        };

        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let background = theme.surface_3();
        let text_color = blended_colors::text_main(theme, background);

        ConstrainedBox::new(
            Container::new(
                Text::new_inline(
                    format!("{zoom_level}%"),
                    appearance.ui_font_family(),
                    HUD_FONT_SIZE,
                )
                .with_style(Properties::default().weight(Weight::Semibold))
                .with_color(text_color)
                .with_selectable(false)
                .finish(),
            )
            .with_horizontal_padding(14.)
            .with_vertical_padding(7.)
            .with_background(background)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(999.)))
            .with_drop_shadow(DropShadow::default())
            .finish(),
        )
        .with_min_width(HUD_MIN_WIDTH)
        .finish()
    }
}

impl Entity for ZoomLevelHud {
    type Event = ();
}

impl TypedActionView for ZoomLevelHud {
    type Action = ();
}
