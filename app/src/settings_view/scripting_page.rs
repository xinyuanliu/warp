use super::{
    settings_page::{
        render_body_item, render_settings_info_banner, LocalOnlyIconState, MatchData, PageType,
        SettingsPageMeta, SettingsPageViewHandle, SettingsWidget,
    },
    SettingsSection, ToggleState,
};
use crate::appearance::Appearance;
use crate::report_if_error;
use crate::settings::{
    AllowInsideWarpControl, AllowInsideWarpLocalMetadata, AllowInsideWarpNonDestructiveMutations,
    AllowOutsideWarpControl, AllowOutsideWarpLocalMetadata,
    AllowOutsideWarpNonDestructiveMutations, LocalControlSettings,
};
use settings::{Setting as _, ToggleableSetting as _};
use std::cell::RefCell;
use std::collections::HashMap;
use warp_core::settings::SyncToCloud;
use warpui::elements::{Element, MouseStateHandle};
use warpui::ui_components::components::UiComponent;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

#[derive(Clone, Copy, Debug)]
pub enum ScriptingToggle {
    AllowInsideWarpControl,
    AllowOutsideWarpControl,
    AllowInsideWarpLocalMetadata,
    AllowOutsideWarpLocalMetadata,
    AllowInsideWarpNonDestructiveMutations,
    AllowOutsideWarpNonDestructiveMutations,
}

impl ScriptingToggle {
    fn label(self) -> &'static str {
        match self {
            Self::AllowInsideWarpControl => "Allow Warp control from inside Warp",
            Self::AllowOutsideWarpControl => "Allow Warp control from outside Warp",
            Self::AllowInsideWarpLocalMetadata => "Allow local read-only metadata from inside Warp",
            Self::AllowOutsideWarpLocalMetadata => {
                "Allow local read-only metadata from outside Warp"
            }
            Self::AllowInsideWarpNonDestructiveMutations => {
                "Allow non-destructive local mutations from inside Warp"
            }
            Self::AllowOutsideWarpNonDestructiveMutations => {
                "Allow non-destructive local mutations from outside Warp"
            }
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::AllowInsideWarpControl => {
                "Allows control commands launched from verified Warp-managed terminal sessions."
            }
            Self::AllowOutsideWarpControl => {
                "Allows other local apps, terminals, IDEs, launch agents, and scripts to request Warp control."
            }
            Self::AllowInsideWarpLocalMetadata => {
                "Allows commands inside Warp to inspect local app metadata such as instances, windows, tabs, and protocol version."
            }
            Self::AllowOutsideWarpLocalMetadata => {
                "Allows external local clients to inspect local app metadata after outside-Warp control is enabled."
            }
            Self::AllowInsideWarpNonDestructiveMutations => {
                "Allows commands inside Warp to make visible, reversible app changes such as creating a tab."
            }
            Self::AllowOutsideWarpNonDestructiveMutations => {
                "Allows external local clients to make visible, reversible app changes after outside-Warp control is enabled."
            }
        }
    }

    fn search_terms(self) -> &'static str {
        match self {
            Self::AllowInsideWarpControl => "inside warp control terminal scripting automation",
            Self::AllowOutsideWarpControl => {
                "outside warp control external scripts automation local cli"
            }
            Self::AllowInsideWarpLocalMetadata => {
                "inside warp local metadata read only windows tabs panes instances"
            }
            Self::AllowOutsideWarpLocalMetadata => {
                "outside warp local metadata read only windows tabs panes instances"
            }
            Self::AllowInsideWarpNonDestructiveMutations => {
                "inside warp non destructive mutations tab create"
            }
            Self::AllowOutsideWarpNonDestructiveMutations => {
                "outside warp non destructive mutations tab create"
            }
        }
    }

    fn value(self, settings: &LocalControlSettings) -> bool {
        match self {
            Self::AllowInsideWarpControl => *settings.allow_inside_warp_control,
            Self::AllowOutsideWarpControl => *settings.allow_outside_warp_control,
            Self::AllowInsideWarpLocalMetadata => *settings.allow_inside_warp_local_metadata,
            Self::AllowOutsideWarpLocalMetadata => *settings.allow_outside_warp_local_metadata,
            Self::AllowInsideWarpNonDestructiveMutations => {
                *settings.allow_inside_warp_non_destructive_mutations
            }
            Self::AllowOutsideWarpNonDestructiveMutations => {
                *settings.allow_outside_warp_non_destructive_mutations
            }
        }
    }

    fn storage_key(self) -> &'static str {
        match self {
            Self::AllowInsideWarpControl => AllowInsideWarpControl::storage_key(),
            Self::AllowOutsideWarpControl => AllowOutsideWarpControl::storage_key(),
            Self::AllowInsideWarpLocalMetadata => AllowInsideWarpLocalMetadata::storage_key(),
            Self::AllowOutsideWarpLocalMetadata => AllowOutsideWarpLocalMetadata::storage_key(),
            Self::AllowInsideWarpNonDestructiveMutations => {
                AllowInsideWarpNonDestructiveMutations::storage_key()
            }
            Self::AllowOutsideWarpNonDestructiveMutations => {
                AllowOutsideWarpNonDestructiveMutations::storage_key()
            }
        }
    }

    fn sync_to_cloud(self) -> SyncToCloud {
        match self {
            Self::AllowInsideWarpControl => AllowInsideWarpControl::sync_to_cloud(),
            Self::AllowOutsideWarpControl => AllowOutsideWarpControl::sync_to_cloud(),
            Self::AllowInsideWarpLocalMetadata => AllowInsideWarpLocalMetadata::sync_to_cloud(),
            Self::AllowOutsideWarpLocalMetadata => AllowOutsideWarpLocalMetadata::sync_to_cloud(),
            Self::AllowInsideWarpNonDestructiveMutations => {
                AllowInsideWarpNonDestructiveMutations::sync_to_cloud()
            }
            Self::AllowOutsideWarpNonDestructiveMutations => {
                AllowOutsideWarpNonDestructiveMutations::sync_to_cloud()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum ScriptingSettingsPageAction {
    Toggle(ScriptingToggle),
}

pub struct ScriptingSettingsPageView {
    page: PageType<Self>,
    local_only_icon_tooltip_states: RefCell<HashMap<String, MouseStateHandle>>,
}

impl ScriptingSettingsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(&LocalControlSettings::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        Self {
            page: PageType::new_uncategorized(
                vec![
                    Box::new(ScriptingIntroWidget),
                    Box::new(ScriptingToggleWidget::new(
                        ScriptingToggle::AllowInsideWarpControl,
                    )),
                    Box::new(ScriptingToggleWidget::new(
                        ScriptingToggle::AllowOutsideWarpControl,
                    )),
                    Box::new(ScriptingToggleWidget::new(
                        ScriptingToggle::AllowInsideWarpLocalMetadata,
                    )),
                    Box::new(ScriptingToggleWidget::new(
                        ScriptingToggle::AllowOutsideWarpLocalMetadata,
                    )),
                    Box::new(ScriptingToggleWidget::new(
                        ScriptingToggle::AllowInsideWarpNonDestructiveMutations,
                    )),
                    Box::new(ScriptingToggleWidget::new(
                        ScriptingToggle::AllowOutsideWarpNonDestructiveMutations,
                    )),
                ],
                Some("Scripting"),
            ),
            local_only_icon_tooltip_states: RefCell::new(HashMap::new()),
        }
    }
}

impl Entity for ScriptingSettingsPageView {
    type Event = ();
}

impl TypedActionView for ScriptingSettingsPageView {
    type Action = ScriptingSettingsPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ScriptingSettingsPageAction::Toggle(toggle) => {
                LocalControlSettings::handle(ctx).update(ctx, |settings, ctx| match toggle {
                    ScriptingToggle::AllowInsideWarpControl => {
                        report_if_error!(settings
                            .allow_inside_warp_control
                            .toggle_and_save_value(ctx));
                    }
                    ScriptingToggle::AllowOutsideWarpControl => {
                        report_if_error!(settings
                            .allow_outside_warp_control
                            .toggle_and_save_value(ctx));
                    }
                    ScriptingToggle::AllowInsideWarpLocalMetadata => {
                        report_if_error!(settings
                            .allow_inside_warp_local_metadata
                            .toggle_and_save_value(ctx));
                    }
                    ScriptingToggle::AllowOutsideWarpLocalMetadata => {
                        report_if_error!(settings
                            .allow_outside_warp_local_metadata
                            .toggle_and_save_value(ctx));
                    }
                    ScriptingToggle::AllowInsideWarpNonDestructiveMutations => {
                        report_if_error!(settings
                            .allow_inside_warp_non_destructive_mutations
                            .toggle_and_save_value(ctx));
                    }
                    ScriptingToggle::AllowOutsideWarpNonDestructiveMutations => {
                        report_if_error!(settings
                            .allow_outside_warp_non_destructive_mutations
                            .toggle_and_save_value(ctx));
                    }
                });
                ctx.notify();
            }
        }
    }
}

impl View for ScriptingSettingsPageView {
    fn ui_name() -> &'static str {
        "ScriptingSettingsPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for ScriptingSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::Scripting
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        cfg!(not(target_family = "wasm"))
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<ScriptingSettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<ScriptingSettingsPageView>) -> Self {
        SettingsPageViewHandle::Scripting(view_handle)
    }
}

struct ScriptingIntroWidget;

impl SettingsWidget for ScriptingIntroWidget {
    type View = ScriptingSettingsPageView;

    fn search_terms(&self) -> &str {
        "scripting warp control automation warpctrl local cli inside outside"
    }

    fn render(
        &self,
        _view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        render_settings_info_banner(
            "Warp control lets local scripts automate allowlisted actions in a running Warp app.",
            Some("Inside-Warp control is scoped to commands launched from Warp-managed terminals. Outside-Warp control allows other local apps and scripts to talk to Warp's control plane."),
            appearance,
        )
    }
}

struct ScriptingToggleWidget {
    toggle: ScriptingToggle,
    switch_state: SwitchStateHandle,
}

impl ScriptingToggleWidget {
    fn new(toggle: ScriptingToggle) -> Self {
        Self {
            toggle,
            switch_state: SwitchStateHandle::default(),
        }
    }
}

impl SettingsWidget for ScriptingToggleWidget {
    type View = ScriptingSettingsPageView;

    fn search_terms(&self) -> &str {
        self.toggle.search_terms()
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let settings = LocalControlSettings::as_ref(app);
        let checked = self.toggle.value(settings);
        let toggle = self.toggle;

        render_body_item::<ScriptingSettingsPageAction>(
            self.toggle.label().to_owned(),
            None,
            LocalOnlyIconState::for_setting(
                self.toggle.storage_key(),
                self.toggle.sync_to_cloud(),
                &mut view.local_only_icon_tooltip_states.borrow_mut(),
                app,
            ),
            ToggleState::Enabled,
            appearance,
            appearance
                .ui_builder()
                .switch(self.switch_state.clone())
                .check(checked)
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(ScriptingSettingsPageAction::Toggle(toggle));
                })
                .finish(),
            Some(self.toggle.description().to_owned()),
        )
    }
}
