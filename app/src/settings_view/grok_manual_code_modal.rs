use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Border, ChildView, Container, CornerRadius, Dismiss, Empty, Flex, ParentElement, Radius, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::ui_components::components::UiComponent;
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
use crate::settings_view::ai_page::AISettingsPageAction;
use crate::ui_components::dialog::{dialog_styles, Dialog};
use crate::view_components::action_button::{ActionButton, NakedTheme, PrimaryTheme};
use crate::view_components::editor_view::{EditorView, SingleLineEditorOptions, TextOptions};

const DIALOG_WIDTH: f32 = 480.;

/// Lightweight snapshot of a Grok OAuth attempt that is waiting for either
/// the loopback callback or a manually-pasted code from xAI's consent screen.
#[derive(Clone, Debug)]
pub struct PendingGrokOauthAttempt {
    pub authorize_url: String,
    pub verifier: String,
}

impl PendingGrokOauthAttempt {
    pub fn new(authorize_url: String, verifier: String) -> Self {
        Self {
            authorize_url,
            verifier,
        }
    }
}

pub enum GrokManualCodeModalEvent {
    Cancel,
    Submit(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GrokManualCodeModalAction {
    Cancel,
    Submit,
}

pub struct GrokManualCodeModal {
    visible: bool,
    authorize_url: Option<String>,
    code_editor: ViewHandle<EditorView>,
    cancel_button: ViewHandle<ActionButton>,
    submit_button: ViewHandle<ActionButton>,
}

impl GrokManualCodeModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let appearance = Appearance::as_ref(ctx);

        let code_editor = ctx.add_typed_action_view(move |ctx| {
            let options = SingleLineEditorOptions {
                is_password: false,
                text: TextOptions {
                    font_size_override: Some(appearance.ui_font_size()),
                    font_family_override: Some(appearance.monospace_font_family()),
                    text_colors_override: Some(warpui::elements::text::TextColors {
                        default_color: appearance.theme().active_ui_text_color(),
                        disabled_color: appearance.theme().disabled_ui_text_color(),
                        hint_color: appearance.theme().disabled_ui_text_color(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut editor = EditorView::single_line(options, ctx);
            editor.set_placeholder_text("Paste code from xAI here", ctx);
            editor
        });

        let cancel_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Cancel", NakedTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(GrokManualCodeModalAction::Cancel);
            })
        });

        let submit_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Connect", PrimaryTheme).on_click(|ctx| {
                ctx.dispatch_typed_action(GrokManualCodeModalAction::Submit);
            })
        });

        Self {
            visible: false,
            authorize_url: None,
            code_editor,
            cancel_button,
            submit_button,
        }
    }

    pub fn show(&mut self, authorize_url: String, ctx: &mut ViewContext<Self>) {
        self.authorize_url = Some(authorize_url);
        self.visible = true;
        // Clear any previous text in the editor
        self.code_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text("", ctx);
        });
        ctx.notify();
    }

    pub fn hide(&mut self, ctx: &mut ViewContext<Self>) {
        self.visible = false;
        self.authorize_url = None;
        ctx.notify();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

impl Entity for GrokManualCodeModal {
    type Event = GrokManualCodeModalEvent;
}

impl View for GrokManualCodeModal {
    fn ui_name() -> &'static str {
        "GrokManualCodeModal"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        if !self.visible {
            return Empty::new().finish();
        }

        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let title = Text::new_inline(
            "Finish connecting SuperGrok",
            appearance.ui_font_family(),
            appearance.ui_font_size(),
        )
        .with_style(Properties::default().weight(Weight::Semibold))
        .with_color(theme.active_ui_text_color().into())
        .finish();

        let description = Text::new(
            "xAI showed a code on the sign-in page. Paste it below to finish connecting your SuperGrok subscription.",
            appearance.ui_font_family(),
            appearance.ui_font_size(),
        )
        .with_color(theme.secondary_ui_text_color().into())
        .soft_wrap(true)
        .finish();

        let url_row = if let Some(url) = &self.authorize_url {
            use crate::workspace::WorkspaceAction;
            let link = crate::view_components::link::Link::new(
                "Re-open authorization page".to_string(),
                crate::settings_view::ai_page::AISettingsPageAction::OpenUrl(url.clone()),
            );
            // The Link type in this codebase often expects specific handling; fall back to a simple text + copy affordance.
            // To keep it simple and robust, show the URL as copyable text and a small helper.
            let url_text = Text::new_inline(
                url.clone(),
                appearance.monospace_font_family(),
                appearance.ui_font_size(),
            )
            .with_color(theme.accent().into_solid())
            .finish();

            Flex::row()
                .with_spacing(8.)
                .with_child(
                    Text::new_inline(
                        "If needed:",
                        appearance.ui_font_family(),
                        appearance.ui_font_size(),
                    )
                    .with_color(theme.secondary_ui_text_color().into())
                    .finish(),
                )
                .with_child(url_text)
                .finish()
        } else {
            Empty::new().finish()
        };

        let input_container = Container::new(ChildView::new(&self.code_editor).finish())
            .with_uniform_padding(8.)
            .with_background(internal_colors::fg_overlay_1(theme))
            .with_border(Border::all(1.).with_border_fill(internal_colors::fg_overlay_3(theme)))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .finish();

        let dialog = Dialog::new(
            "Enter code from xAI".to_string(),
            None,
            dialog_styles(appearance),
        )
        .with_child(
            Flex::column()
                .with_spacing(12.)
                .with_child(title)
                .with_child(description)
                .with_child(input_container)
                .with_child(url_row)
                .finish(),
        )
        .with_bottom_row_child(ChildView::new(&self.cancel_button).finish())
        .with_bottom_row_child(
            Container::new(ChildView::new(&self.submit_button).finish())
                .with_margin_left(12.)
                .finish(),
        )
        .with_width(DIALOG_WIDTH)
        .build()
        .finish();

        Dismiss::new(dialog)
            .prevent_interaction_with_other_elements()
            .on_dismiss(|ctx, _app| ctx.dispatch_typed_action(GrokManualCodeModalAction::Cancel))
            .finish()
    }
}

impl TypedActionView for GrokManualCodeModal {
    type Action = GrokManualCodeModalAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            GrokManualCodeModalAction::Cancel => {
                ctx.emit(GrokManualCodeModalEvent::Cancel);
            }
            GrokManualCodeModalAction::Submit => {
                let code = self.code_editor.as_ref(ctx).buffer_text(ctx);
                if !code.trim().is_empty() {
                    ctx.emit(GrokManualCodeModalEvent::Submit(code));
                }
            }
        }
    }
}
