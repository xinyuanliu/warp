use ::local_control::auth::CredentialGrant;
use ::local_control::protocol::ActionKind;
use ::local_control::protocol::{
    Action, ControlResponse, PaneSelector, PaneTarget, SessionSelector, SessionTarget, TabSelector,
    TabTarget, TargetSelector, WindowSelector, WindowTarget,
};
use ::local_control::{
    ErrorCode, InstanceId, InvocationContext, PermissionCategory, RequestEnvelope,
};
use chrono::Duration;
use settings::Setting as _;
use warp_core::features::FeatureFlag;
use warpui::{App, SingletonEntity};

use super::{
    action_metadata_for_name, appearance_state_result, capabilities, ensure_feature_enabled,
    ensure_scripting_grant_for_settings, ensure_settings_allow_action,
    outside_warp_action_enabled_for_settings, rejected_setting_key, require_active_window_id,
    setting_get_result, setting_list_result, theme_list_result, validate_action_params,
    validate_app_focus_target_test, validate_tab_create_target, validate_window_create_target_test,
    LocalControlBridge,
};
use crate::settings::{
    AllowOutsideWarpAppStateMutations, AllowOutsideWarpAuthenticatedUserActions,
    AllowOutsideWarpControl, AllowOutsideWarpMetadataConfigurationMutations,
    AllowOutsideWarpMetadataReads, AllowOutsideWarpUnderlyingDataMutations,
    AllowOutsideWarpUnderlyingDataReads, LocalControlSettings,
};
use crate::test_util::settings::initialize_settings_for_tests;
use ::local_control::scripting::{ScriptingGrant, ScriptingIdentitySource, ScriptingScope};

fn settings_with_values(
    outside_control: bool,
    outside_metadata_reads: bool,
    outside_underlying_data_reads: bool,
    outside_app_state_mutations: bool,
    outside_metadata_configuration_mutations: bool,
    outside_underlying_data_mutations: bool,
) -> LocalControlSettings {
    LocalControlSettings {
        allow_outside_warp_control: AllowOutsideWarpControl::new(Some(outside_control)),
        allow_outside_warp_metadata_reads: AllowOutsideWarpMetadataReads::new(Some(
            outside_metadata_reads,
        )),
        allow_outside_warp_underlying_data_reads: AllowOutsideWarpUnderlyingDataReads::new(Some(
            outside_underlying_data_reads,
        )),
        allow_outside_warp_app_state_mutations: AllowOutsideWarpAppStateMutations::new(Some(
            outside_app_state_mutations,
        )),
        allow_outside_warp_metadata_configuration_mutations:
            AllowOutsideWarpMetadataConfigurationMutations::new(Some(
                outside_metadata_configuration_mutations,
            )),
        allow_outside_warp_underlying_data_mutations: AllowOutsideWarpUnderlyingDataMutations::new(
            Some(outside_underlying_data_mutations),
        ),
        allow_outside_warp_authenticated_user_actions:
            AllowOutsideWarpAuthenticatedUserActions::new(Some(false)),
    }
}

fn settings_with_authenticated_user_actions(
    outside_control: bool,
    outside_underlying_data_mutations: bool,
    authenticated_user_actions: bool,
) -> LocalControlSettings {
    LocalControlSettings {
        allow_outside_warp_control: AllowOutsideWarpControl::new(Some(outside_control)),
        allow_outside_warp_metadata_reads: AllowOutsideWarpMetadataReads::new(Some(false)),
        allow_outside_warp_underlying_data_reads: AllowOutsideWarpUnderlyingDataReads::new(Some(
            false,
        )),
        allow_outside_warp_app_state_mutations: AllowOutsideWarpAppStateMutations::new(Some(false)),
        allow_outside_warp_metadata_configuration_mutations:
            AllowOutsideWarpMetadataConfigurationMutations::new(Some(false)),
        allow_outside_warp_underlying_data_mutations: AllowOutsideWarpUnderlyingDataMutations::new(
            Some(outside_underlying_data_mutations),
        ),
        allow_outside_warp_authenticated_user_actions:
            AllowOutsideWarpAuthenticatedUserActions::new(Some(authenticated_user_actions)),
    }
}

fn scripting_grant() -> ScriptingGrant {
    ScriptingGrant {
        source: ScriptingIdentitySource::ExternalApiKey {
            key_id: "kid_test".to_owned(),
        },
        subject: "test-user".to_owned(),
        scopes: vec![ScriptingScope::LocalControlMutateUnderlyingData],
        issued_at: chrono::Utc::now(),
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
    }
}

fn settings_with_outside_warp(
    outside_control: bool,
    outside_app_state_mutations: bool,
) -> LocalControlSettings {
    settings_with_values(
        outside_control,
        false,
        false,
        outside_app_state_mutations,
        false,
        false,
    )
}

fn enable_outside_warp_metadata_reads(app: &mut App) {
    app.update(|ctx| {
        LocalControlSettings::handle(ctx).update(ctx, |settings, ctx| {
            let _ = settings.allow_outside_warp_control.set_value(true, ctx);
            let _ = settings
                .allow_outside_warp_metadata_reads
                .set_value(true, ctx);
        });
    });
}
fn grant_for(action: ActionKind) -> CredentialGrant {
    CredentialGrant::new(
        InstanceId("test-instance".to_owned()),
        action,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    )
}

fn request_with_target(action: ActionKind, target: TargetSelector) -> RequestEnvelope {
    let mut request = RequestEnvelope::new(Action::new(action));
    request.target = target;
    request
}

fn response_error_code(response: ::local_control::ResponseEnvelope) -> ErrorCode {
    match response.response {
        ControlResponse::Error { error } => error.code,
        ControlResponse::Ok { data } => panic!("expected error response, got {data:?}"),
    }
}

#[test]
fn tab_create_accepts_default_and_active_targets() {
    validate_tab_create_target(&TargetSelector::default()).expect("default target is accepted");

    validate_tab_create_target(&TargetSelector {
        window: Some(WindowTarget::Active),
        tab: Some(TabTarget::Active),
        pane: Some(PaneTarget::Active),
        session: Some(SessionTarget::Active),
        ..TargetSelector::default()
    })
    .expect("active target is accepted");
}

#[test]
fn tab_create_rejects_concrete_targets() {
    let err = validate_tab_create_target(&TargetSelector {
        window: Some(WindowTarget::Id {
            id: WindowSelector("window".to_owned()),
        }),
        ..TargetSelector::default()
    })
    .expect_err("concrete window target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_tab_create_target(&TargetSelector {
        tab: Some(TabTarget::Id {
            id: TabSelector("tab".to_owned()),
        }),
        ..TargetSelector::default()
    })
    .expect_err("concrete tab target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_tab_create_target(&TargetSelector {
        pane: Some(PaneTarget::Id {
            id: PaneSelector("pane".to_owned()),
        }),
        ..TargetSelector::default()
    })
    .expect_err("concrete pane target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_tab_create_target(&TargetSelector {
        session: Some(SessionTarget::Id {
            id: SessionSelector("session".to_owned()),
        }),
        ..TargetSelector::default()
    })
    .expect_err("concrete session target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);
}

#[test]
fn tab_create_rejects_unsupported_selector_forms() {
    let err = validate_tab_create_target(&TargetSelector {
        window: Some(WindowTarget::Index { index: 0 }),
        ..TargetSelector::default()
    })
    .expect_err("indexed window target is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    let err = validate_tab_create_target(&TargetSelector {
        tab: Some(TabTarget::Index { index: 0 }),
        ..TargetSelector::default()
    })
    .expect_err("indexed tab target is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn capabilities_advertises_core_metadata_and_layout_mutation_actions() {
    let caps = capabilities();
    assert!(caps.contains(&ActionKind::InstanceList));
    assert!(caps.contains(&ActionKind::AppPing));
    assert!(caps.contains(&ActionKind::WindowList));
    assert!(caps.contains(&ActionKind::TabList));
    assert!(caps.contains(&ActionKind::TabCreate));
    assert!(caps.contains(&ActionKind::PaneList));
    assert!(caps.contains(&ActionKind::BlockList));
    assert!(caps.contains(&ActionKind::HistoryList));
    assert!(caps.contains(&ActionKind::AppFocus));
    assert!(caps.contains(&ActionKind::WindowCreate));
    assert!(caps.contains(&ActionKind::WindowFocus));
    assert!(caps.contains(&ActionKind::WindowClose));
    assert!(caps.contains(&ActionKind::TabActivate));
    assert!(caps.contains(&ActionKind::TabMove));
    assert!(caps.contains(&ActionKind::TabClose));
    assert!(caps.contains(&ActionKind::PaneSplit));
    assert!(caps.contains(&ActionKind::PaneFocus));
    assert!(caps.contains(&ActionKind::PaneNavigate));
    assert!(caps.contains(&ActionKind::PaneClose));
    assert!(caps.contains(&ActionKind::PaneMaximize));
    assert!(caps.contains(&ActionKind::PaneResize));
    assert!(!caps.contains(&ActionKind::TabRename));
    assert!(!caps.contains(&ActionKind::InputRun));
}

#[test]
fn capabilities_advertises_session_and_input_mutation_actions() {
    let caps = capabilities();
    assert!(caps.contains(&ActionKind::SessionActivate));
    assert!(caps.contains(&ActionKind::SessionPrevious));
    assert!(caps.contains(&ActionKind::SessionNext));
    assert!(caps.contains(&ActionKind::SessionReopen));
    assert!(caps.contains(&ActionKind::InputInsert));
    assert!(caps.contains(&ActionKind::InputReplace));
    assert!(caps.contains(&ActionKind::InputClear));
    assert!(caps.contains(&ActionKind::InputModeSet));
}

#[test]
fn capabilities_advertises_settings_and_surface_mutation_actions() {
    let caps = capabilities();
    assert!(caps.contains(&ActionKind::ThemeSet));
    assert!(caps.contains(&ActionKind::AppearanceSet));
    assert!(caps.contains(&ActionKind::AppearanceFontSize));
    assert!(caps.contains(&ActionKind::AppearanceZoom));
    assert!(caps.contains(&ActionKind::SettingSet));
    assert!(caps.contains(&ActionKind::SettingToggle));
    assert!(caps.contains(&ActionKind::AppSettingsOpen));
    assert!(caps.contains(&ActionKind::AppCommandPaletteOpen));
    assert!(caps.contains(&ActionKind::AppCommandSearchOpen));
    assert!(caps.contains(&ActionKind::AppWarpDriveOpen));
    assert!(caps.contains(&ActionKind::AppWarpDriveToggle));
    assert!(caps.contains(&ActionKind::AppResourceCenterToggle));
    assert!(caps.contains(&ActionKind::AppAiAssistantToggle));
    assert!(caps.contains(&ActionKind::AppCodeReviewToggle));
    assert!(caps.contains(&ActionKind::AppVerticalTabsToggle));
}

#[test]
fn outside_warp_discovery_requires_context_and_action_permission() {
    assert!(!outside_warp_action_enabled_for_settings(
        &settings_with_outside_warp(false, true),
        ActionKind::TabCreate
    ));
    assert!(!outside_warp_action_enabled_for_settings(
        &settings_with_outside_warp(true, false),
        ActionKind::TabCreate
    ));
    assert!(outside_warp_action_enabled_for_settings(
        &settings_with_outside_warp(true, true),
        ActionKind::TabCreate
    ));
    assert!(!outside_warp_action_enabled_for_settings(
        &settings_with_values(true, false, false, true, false, false),
        ActionKind::WindowList
    ));
    assert!(outside_warp_action_enabled_for_settings(
        &settings_with_values(true, true, false, false, false, false),
        ActionKind::WindowList
    ));
}

#[test]
fn tab_create_requires_active_window() {
    let active = warpui::WindowId::from_usize(1);

    assert_eq!(
        require_active_window_id(Some(active)).expect("active"),
        active
    );
    let err = require_active_window_id(None).expect_err("missing active window");
    assert_eq!(err.code, ErrorCode::MissingTarget);
}

#[test]
fn feature_flag_disabled_denies_local_control() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(false);
    let err = ensure_feature_enabled().expect_err("feature flag disabled");
    assert_eq!(err.code, ErrorCode::LocalControlDisabled);
}

#[test]
fn disabled_outside_warp_denies_before_granular_permission() {
    let settings = settings_with_values(false, true, false, true, false, false);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::OutsideWarp,
        ActionKind::TabCreate,
    )
    .expect_err("outside-Warp parent context is disabled");
    assert_eq!(err.code, ErrorCode::LocalControlDisabled);
}

#[test]
fn inside_warp_context_is_not_implemented() {
    let settings = settings_with_values(true, true, false, true, false, false);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::InsideWarp,
        ActionKind::TabCreate,
    )
    .expect_err("inside-Warp grants are not implemented");
    assert_eq!(err.code, ErrorCode::ExecutionContextNotAllowed);
}

#[test]
fn disabled_granular_permission_denies_with_insufficient_permissions() {
    let settings = settings_with_values(true, true, false, false, false, false);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::OutsideWarp,
        ActionKind::TabCreate,
    )
    .expect_err("read-write permission is disabled");
    assert_eq!(err.code, ErrorCode::InsufficientPermissions);
}

#[test]
fn tab_create_rejects_malformed_params() {
    let err = validate_action_params(&Action {
        kind: ActionKind::TabCreate,
        params: serde_json::json!({ "unexpected": true }),
    })
    .expect_err("tab.create params must be empty");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::TabCreate,
        params: serde_json::json!({}),
    })
    .expect("empty tab.create params are accepted");
}

#[test]
fn metadata_handlers_return_successful_empty_metadata_without_windows() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        enable_outside_warp_metadata_reads(&mut app);
        let bridge = app.add_model(LocalControlBridge::new);

        for action in [
            ActionKind::AppActive,
            ActionKind::AppInspect,
            ActionKind::AppVersion,
            ActionKind::ActionList,
            ActionKind::WindowList,
            ActionKind::TabList,
            ActionKind::PaneList,
            ActionKind::SessionList,
        ] {
            let response = bridge.update(&mut app, |bridge, ctx| {
                bridge.handle_request(
                    RequestEnvelope::new(Action::new(action)),
                    grant_for(action),
                    ctx,
                )
            });
            match response.response {
                ControlResponse::Ok { data } => {
                    assert_eq!(data["action"], action.as_str());
                }
                ControlResponse::Error { error } => {
                    panic!("{} returned {error}", action.as_str());
                }
            }
        }
    });
}

#[test]
fn metadata_list_handlers_reject_stale_and_unsupported_selectors() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        enable_outside_warp_metadata_reads(&mut app);
        let bridge = app.add_model(LocalControlBridge::new);

        let cases = [
            (
                ActionKind::WindowList,
                TargetSelector {
                    tab: Some(TabTarget::Active),
                    ..TargetSelector::default()
                },
                ErrorCode::InvalidSelector,
            ),
            (
                ActionKind::WindowList,
                TargetSelector {
                    window: Some(WindowTarget::Id {
                        id: WindowSelector("stale-window".to_owned()),
                    }),
                    ..TargetSelector::default()
                },
                ErrorCode::StaleTarget,
            ),
            (
                ActionKind::TabList,
                TargetSelector {
                    tab: Some(TabTarget::Title {
                        title: "unsupported".to_owned(),
                    }),
                    ..TargetSelector::default()
                },
                ErrorCode::InvalidSelector,
            ),
            (
                ActionKind::PaneList,
                TargetSelector {
                    pane: Some(PaneTarget::Id {
                        id: PaneSelector("stale-pane".to_owned()),
                    }),
                    ..TargetSelector::default()
                },
                ErrorCode::StaleTarget,
            ),
            (
                ActionKind::SessionList,
                TargetSelector {
                    session: Some(SessionTarget::Id {
                        id: SessionSelector("stale-session".to_owned()),
                    }),
                    ..TargetSelector::default()
                },
                ErrorCode::StaleTarget,
            ),
        ];

        for (action, target, code) in cases {
            let response = bridge.update(&mut app, |bridge, ctx| {
                bridge.handle_request(request_with_target(action, target), grant_for(action), ctx)
            });
            assert_eq!(response_error_code(response), code);
        }
    });
}

#[test]
fn metadata_actions_require_metadata_permission_not_app_state_mutation_permission() {
    let metadata_without_mutation = settings_with_values(true, true, false, false, false, false);
    let mutation_without_metadata = settings_with_values(true, false, false, true, false, false);

    for action in [
        ActionKind::InstanceList,
        ActionKind::AppPing,
        ActionKind::AppInspect,
        ActionKind::AppVersion,
        ActionKind::AppActive,
        ActionKind::ActionList,
        ActionKind::ActionGet,
        ActionKind::WindowList,
        ActionKind::TabList,
        ActionKind::PaneList,
        ActionKind::SessionList,
        ActionKind::ThemeList,
        ActionKind::AppearanceGet,
        ActionKind::SettingGet,
        ActionKind::SettingList,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::ReadMetadata
        );
        ensure_settings_allow_action(
            &metadata_without_mutation,
            InvocationContext::OutsideWarp,
            action,
        )
        .expect("metadata read permission allows metadata action");
        let err = ensure_settings_allow_action(
            &mutation_without_metadata,
            InvocationContext::OutsideWarp,
            action,
        )
        .expect_err("metadata action is denied without metadata read permission");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }

    assert_eq!(
        ActionKind::TabCreate.metadata().permission_category,
        PermissionCategory::MutateAppState
    );
    ensure_settings_allow_action(
        &mutation_without_metadata,
        InvocationContext::OutsideWarp,
        ActionKind::TabCreate,
    )
    .expect("app-state mutation permission allows tab.create");
}

#[test]
fn data_actions_require_underlying_data_permission_not_metadata_permission() {
    let underlying_data_without_metadata =
        settings_with_values(true, false, true, false, false, false);
    let metadata_without_underlying_data =
        settings_with_values(true, true, false, false, false, false);

    for action in [
        ActionKind::BlockList,
        ActionKind::BlockGet,
        ActionKind::InputGet,
        ActionKind::HistoryList,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::ReadUnderlyingData
        );
        ensure_settings_allow_action(
            &underlying_data_without_metadata,
            InvocationContext::OutsideWarp,
            action,
        )
        .expect("underlying data read permission allows data action");
        let err = ensure_settings_allow_action(
            &metadata_without_underlying_data,
            InvocationContext::OutsideWarp,
            action,
        )
        .expect_err("data action is denied without underlying data read permission");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }
}

#[test]
fn action_get_rejects_unallowlisted_action_names() {
    let err = validate_action_params(&Action {
        kind: ActionKind::ActionGet,
        params: serde_json::json!({ "action": "input.execute" }),
    })
    .expect_err("unallowlisted action is rejected");
    assert_eq!(err.code, ErrorCode::NotAllowlisted);
}

#[test]
fn action_metadata_lookup_reports_implemented_status_for_layout_mutations() {
    let metadata = action_metadata_for_name("window.create").expect("allowlisted action");
    assert_eq!(metadata.kind, ActionKind::WindowCreate);
    assert_eq!(
        metadata.implementation_status,
        ::local_control::ActionImplementationStatus::Implemented
    );

    let metadata = action_metadata_for_name("pane.split").expect("allowlisted action");
    assert_eq!(metadata.kind, ActionKind::PaneSplit);
    assert_eq!(
        metadata.implementation_status,
        ::local_control::ActionImplementationStatus::Implemented
    );
}

#[test]
fn action_metadata_lookup_reports_stub_status_for_deferred_actions() {
    let metadata = action_metadata_for_name("tab.rename").expect("allowlisted action");
    assert_eq!(metadata.kind, ActionKind::TabRename);
    assert_eq!(
        metadata.implementation_status,
        ::local_control::ActionImplementationStatus::Stub
    );
}

#[test]
fn app_target_metadata_reads_reject_malformed_params() {
    for action in [
        ActionKind::AppVersion,
        ActionKind::AppActive,
        ActionKind::AppInspect,
        ActionKind::ActionList,
        ActionKind::WindowList,
        ActionKind::TabList,
        ActionKind::PaneList,
        ActionKind::SessionList,
        ActionKind::ThemeList,
        ActionKind::AppearanceGet,
        ActionKind::SettingList,
    ] {
        let err = validate_action_params(&Action {
            kind: action,
            params: serde_json::json!({ "unexpected": true }),
        })
        .expect_err("app target metadata read params must be empty");
        assert_eq!(err.code, ErrorCode::InvalidParams);

        validate_action_params(&Action {
            kind: action,
            params: serde_json::json!({}),
        })
        .expect("empty app target metadata read params are accepted");
    }

    validate_action_params(&Action {
        kind: ActionKind::SettingGet,
        params: serde_json::json!({ "key": "appearance.themes.theme" }),
    })
    .expect("setting.get accepts a key parameter");
}

#[test]
fn settings_and_appearance_handlers_return_allowlisted_metadata() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let bridge = app.add_model(LocalControlBridge::new);

        bridge.update(&mut app, |_, ctx| {
            let themes = theme_list_result(ctx).expect("themes are listed");
            assert!(themes.themes.iter().any(|theme| theme.name == "Dark"));

            let appearance = appearance_state_result(ctx).expect("appearance is readable");
            assert_eq!(appearance.theme.as_deref(), Some("Dark"));
            assert_eq!(appearance.light_theme.as_deref(), Some("Light"));
            assert_eq!(appearance.dark_theme.as_deref(), Some("Dark"));
            assert_eq!(appearance.ui_zoom_percent, Some(100));

            let settings = setting_list_result(ctx).expect("settings are listed");
            assert!(settings
                .settings
                .iter()
                .any(|setting| setting.key == "appearance.themes.system_theme"));

            let setting = setting_get_result("appearance.themes.system_theme", ctx)
                .expect("allowlisted setting is readable");
            assert_eq!(setting.setting.value, serde_json::json!(false));
            assert_eq!(setting.setting.value_type, "bool");
        });
    });
}

#[test]
fn setting_get_rejects_unknown_and_private_settings() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let bridge = app.add_model(LocalControlBridge::new);

        bridge.update(&mut app, |_, ctx| {
            let err = setting_get_result("appearance.secrets.token", ctx)
                .expect_err("unknown settings are rejected");
            assert_eq!(err.code, ErrorCode::NotAllowlisted);

            let err = setting_get_result("local_control.allow_outside_warp_control", ctx)
                .expect_err("private settings are rejected");
            assert_eq!(err.code, ErrorCode::NotAllowlisted);
            assert!(err.message.contains("private or sensitive"));
        });
    });
}

#[test]
fn rejected_setting_key_distinguishes_private_settings() {
    let private_err = rejected_setting_key("terminal.input.inline_menu_custom_content_heights");
    assert_eq!(private_err.code, ErrorCode::NotAllowlisted);
    assert!(private_err.message.contains("private or sensitive"));

    let unknown_err = rejected_setting_key("terminal.input.not_real");
    assert_eq!(unknown_err.code, ErrorCode::NotAllowlisted);
    assert!(unknown_err.message.contains("not an allowlisted"));
}

#[test]
fn settings_and_appearance_bridge_handlers_return_success() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        enable_outside_warp_metadata_reads(&mut app);
        let bridge = app.add_model(LocalControlBridge::new);

        for action in [
            ActionKind::ThemeList,
            ActionKind::AppearanceGet,
            ActionKind::SettingList,
        ] {
            let response = bridge.update(&mut app, |bridge, ctx| {
                bridge.handle_request(
                    RequestEnvelope::new(Action::new(action)),
                    grant_for(action),
                    ctx,
                )
            });
            match response.response {
                ControlResponse::Ok { data } => assert!(data.is_object()),
                ControlResponse::Error { error } => {
                    panic!("{} returned {error}", action.as_str());
                }
            }
        }

        let action = Action::with_params(
            ActionKind::SettingGet,
            ::local_control::SettingGetParams {
                key: "appearance.themes.system_theme".to_owned(),
            },
        )
        .expect("setting.get params serialize");
        let response = bridge.update(&mut app, |bridge, ctx| {
            bridge.handle_request(
                RequestEnvelope::new(action),
                grant_for(ActionKind::SettingGet),
                ctx,
            )
        });
        match response.response {
            ControlResponse::Ok { data } => {
                assert_eq!(data["setting"]["key"], "appearance.themes.system_theme");
            }
            ControlResponse::Error { error } => {
                panic!("setting.get returned {error}");
            }
        }
    });
}

#[test]
fn data_reads_reject_malformed_params() {
    validate_action_params(&Action {
        kind: ActionKind::InputGet,
        params: serde_json::json!({}),
    })
    .expect("input.get accepts empty params");

    let err = validate_action_params(&Action {
        kind: ActionKind::InputGet,
        params: serde_json::json!({ "unexpected": true }),
    })
    .expect_err("input.get params must be empty");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::BlockList,
        params: serde_json::json!({ "limit": 10 }),
    })
    .expect("block.list accepts limit");

    validate_action_params(&Action {
        kind: ActionKind::HistoryList,
        params: serde_json::json!({ "limit": 20 }),
    })
    .expect("history.list accepts limit");

    let err = validate_action_params(&Action {
        kind: ActionKind::BlockGet,
        params: serde_json::json!({ "block_id": "" }),
    })
    .expect_err("block.get requires a block id");
    assert_eq!(err.code, ErrorCode::InvalidParams);
}

#[test]
fn high_risk_actions_require_authenticated_scripting_grant() {
    let settings_with_auth = settings_with_authenticated_user_actions(true, true, true);
    let settings_without_auth = settings_with_authenticated_user_actions(true, true, false);

    for action in [ActionKind::InputRun] {
        let grant_without_scripting = CredentialGrant::new(
            InstanceId("test-instance".to_owned()),
            action,
            InvocationContext::OutsideWarp,
            Duration::minutes(5),
        );
        let err = ensure_scripting_grant_for_settings(
            &settings_with_auth,
            action,
            &grant_without_scripting,
        )
        .expect_err("high-risk action is denied without scripting grant");
        assert_eq!(
            err.code,
            ErrorCode::AuthenticatedScriptingRequired,
            "{} should require scripting grant",
            action.as_str()
        );

        let err = ensure_scripting_grant_for_settings(
            &settings_without_auth,
            action,
            &grant_without_scripting,
        )
        .expect_err("high-risk action is denied when authenticated user actions are disabled");
        assert_eq!(
            err.code,
            ErrorCode::AuthenticatedScriptingRequired,
            "{} denied when authenticated actions disabled",
            action.as_str()
        );
    }
}

#[test]
fn high_risk_actions_with_scripting_grant_and_enabled_setting_pass_grant_check() {
    let settings_with_auth = settings_with_authenticated_user_actions(true, true, true);

    for action in [ActionKind::InputRun] {
        let mut grant = CredentialGrant::new(
            InstanceId("test-instance".to_owned()),
            action,
            InvocationContext::OutsideWarp,
            Duration::minutes(5),
        );
        grant.scripting_grant = Some(scripting_grant());

        ensure_scripting_grant_for_settings(&settings_with_auth, action, &grant)
            .expect("high-risk action is allowed with scripting grant and enabled setting");
    }
}

#[test]
fn high_risk_actions_with_scripting_grant_but_disabled_setting_are_denied() {
    let settings_without_auth = settings_with_authenticated_user_actions(true, true, false);

    for action in [ActionKind::InputRun] {
        let mut grant = CredentialGrant::new(
            InstanceId("test-instance".to_owned()),
            action,
            InvocationContext::OutsideWarp,
            Duration::minutes(5),
        );
        grant.scripting_grant = Some(scripting_grant());

        let err = ensure_scripting_grant_for_settings(&settings_without_auth, action, &grant)
            .expect_err("scripting grant is denied when authenticated actions setting is off");
        assert_eq!(err.code, ErrorCode::AuthenticatedScriptingRequired);
    }
}

#[test]
fn low_risk_actions_pass_scripting_grant_check_without_grant() {
    let settings_without_auth = settings_with_authenticated_user_actions(true, false, false);

    for action in [
        ActionKind::TabCreate,
        ActionKind::InstanceList,
        ActionKind::AppPing,
        ActionKind::WindowList,
        ActionKind::SettingGet,
        ActionKind::InputInsert,
        ActionKind::InputReplace,
        ActionKind::InputClear,
        ActionKind::InputModeSet,
    ] {
        let grant = CredentialGrant::new(
            InstanceId("test-instance".to_owned()),
            action,
            InvocationContext::OutsideWarp,
            Duration::minutes(5),
        );
        ensure_scripting_grant_for_settings(&settings_without_auth, action, &grant)
            .expect("low-risk action does not need scripting grant");
    }
}

#[test]
fn authenticated_scripting_required_error_code_serializes_stably() {
    use ::local_control::ErrorCode;
    let code = ErrorCode::AuthenticatedScriptingRequired;
    let value = serde_json::to_value(code).expect("serializes");
    assert_eq!(value, serde_json::json!("authenticated_scripting_required"));
}

#[test]
fn layout_mutations_use_mutate_app_state_permission_category() {
    for action in [
        ActionKind::AppFocus,
        ActionKind::WindowCreate,
        ActionKind::WindowFocus,
        ActionKind::WindowClose,
        ActionKind::TabActivate,
        ActionKind::TabMove,
        ActionKind::TabClose,
        ActionKind::PaneSplit,
        ActionKind::PaneFocus,
        ActionKind::PaneNavigate,
        ActionKind::PaneClose,
        ActionKind::PaneMaximize,
        ActionKind::PaneResize,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateAppState,
            "{} should use MutateAppState permission",
            action.as_str()
        );
    }
}

#[test]
fn layout_mutations_require_app_state_mutation_permission_not_other_grants() {
    let app_state_only = settings_with_values(true, false, false, true, false, false);
    let metadata_only = settings_with_values(true, true, false, false, false, false);
    let underlying_data_only = settings_with_values(true, false, true, false, false, false);
    let metadata_config_only = settings_with_values(true, false, false, false, true, false);
    let underlying_mutation_only = settings_with_values(true, false, false, false, false, true);

    for action in [
        ActionKind::AppFocus,
        ActionKind::WindowCreate,
        ActionKind::WindowFocus,
        ActionKind::WindowClose,
        ActionKind::TabActivate,
        ActionKind::TabMove,
        ActionKind::TabClose,
        ActionKind::PaneSplit,
        ActionKind::PaneFocus,
        ActionKind::PaneNavigate,
        ActionKind::PaneClose,
        ActionKind::PaneMaximize,
        ActionKind::PaneResize,
    ] {
        ensure_settings_allow_action(&app_state_only, InvocationContext::OutsideWarp, action)
            .expect("app-state mutation permission allows layout mutation");

        for wrong_settings in [
            &metadata_only,
            &underlying_data_only,
            &metadata_config_only,
            &underlying_mutation_only,
        ] {
            let err = ensure_settings_allow_action(
                wrong_settings,
                InvocationContext::OutsideWarp,
                action,
            )
            .expect_err("layout mutation denied without app-state mutation permission");
            assert_eq!(
                err.code,
                ErrorCode::InsufficientPermissions,
                "{} should require MutateAppState",
                action.as_str()
            );
        }
    }
}

#[test]
fn layout_mutations_require_authenticated_user() {
    for action in [
        ActionKind::AppFocus,
        ActionKind::WindowCreate,
        ActionKind::WindowFocus,
        ActionKind::WindowClose,
        ActionKind::TabActivate,
        ActionKind::TabMove,
        ActionKind::TabClose,
        ActionKind::PaneSplit,
        ActionKind::PaneFocus,
        ActionKind::PaneNavigate,
        ActionKind::PaneClose,
        ActionKind::PaneMaximize,
        ActionKind::PaneResize,
    ] {
        assert!(
            action.metadata().requires_authenticated_user,
            "{} should require authenticated user",
            action.as_str()
        );
    }
    assert!(
        !ActionKind::TabCreate.metadata().requires_authenticated_user,
        "tab.create does not require authenticated user"
    );
}

#[test]
fn close_commands_require_explicit_target_selectors() {
    let err = validate_window_create_target_test(
        &TargetSelector {
            window: Some(WindowTarget::Active),
            ..TargetSelector::default()
        },
        &::local_control::protocol::WindowCreateParams::default(),
    )
    .expect_err("window.create rejects window selector");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    validate_window_create_target_test(
        &TargetSelector::default(),
        &::local_control::protocol::WindowCreateParams::default(),
    )
    .expect("window.create accepts default selector");

    validate_app_focus_target_test(&TargetSelector::default())
        .expect("app.focus accepts default selector");

    let err = validate_app_focus_target_test(&TargetSelector {
        window: Some(WindowTarget::Active),
        ..TargetSelector::default()
    })
    .expect_err("app.focus rejects window selector");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn layout_mutation_params_reject_malformed_inputs() {
    validate_action_params(&Action {
        kind: ActionKind::AppFocus,
        params: serde_json::json!({}),
    })
    .expect("app.focus accepts empty params");

    let err = validate_action_params(&Action {
        kind: ActionKind::AppFocus,
        params: serde_json::json!({ "unexpected": true }),
    })
    .expect_err("app.focus rejects extra params");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::WindowCreate,
        params: serde_json::json!({}),
    })
    .expect("window.create accepts empty params");

    validate_action_params(&Action {
        kind: ActionKind::WindowCreate,
        params: serde_json::json!({ "profile": null }),
    })
    .expect("window.create accepts null profile");

    validate_action_params(&Action {
        kind: ActionKind::TabActivate,
        params: serde_json::json!({}),
    })
    .expect("tab.activate accepts empty params");

    validate_action_params(&Action {
        kind: ActionKind::TabActivate,
        params: serde_json::json!({ "relative": "next" }),
    })
    .expect("tab.activate accepts relative param");

    let err = validate_action_params(&Action {
        kind: ActionKind::TabMove,
        params: serde_json::json!({}),
    })
    .expect_err("tab.move requires a direction");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::TabMove,
        params: serde_json::json!({ "direction": "left" }),
    })
    .expect("tab.move accepts direction");

    let err = validate_action_params(&Action {
        kind: ActionKind::PaneSplit,
        params: serde_json::json!({}),
    })
    .expect_err("pane.split requires a direction");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::PaneSplit,
        params: serde_json::json!({ "direction": "right" }),
    })
    .expect("pane.split accepts direction");

    let err = validate_action_params(&Action {
        kind: ActionKind::PaneNavigate,
        params: serde_json::json!({}),
    })
    .expect_err("pane.navigate requires a direction");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    let err = validate_action_params(&Action {
        kind: ActionKind::PaneResize,
        params: serde_json::json!({ "direction": "up", "amount": 0 }),
    })
    .expect_err("pane.resize rejects zero amount");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::PaneResize,
        params: serde_json::json!({ "direction": "up", "amount": 3 }),
    })
    .expect("pane.resize accepts positive amount");

    validate_action_params(&Action {
        kind: ActionKind::PaneMaximize,
        params: serde_json::json!({}),
    })
    .expect("pane.maximize accepts empty params");

    validate_action_params(&Action {
        kind: ActionKind::PaneMaximize,
        params: serde_json::json!({ "enabled": true }),
    })
    .expect("pane.maximize accepts enabled param");

    validate_action_params(&Action {
        kind: ActionKind::PaneFocus,
        params: serde_json::json!({}),
    })
    .expect("pane.focus accepts empty params");

    validate_action_params(&Action {
        kind: ActionKind::PaneClose,
        params: serde_json::json!({}),
    })
    .expect("pane.close accepts empty params");
}

#[test]
fn session_mutation_actions_use_app_state_mutation_permission_category() {
    for action in [
        ActionKind::SessionActivate,
        ActionKind::SessionPrevious,
        ActionKind::SessionNext,
        ActionKind::SessionReopen,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateAppState,
            "{} should use MutateAppState permission",
            action.as_str()
        );
    }
}

#[test]
fn input_staging_mutations_are_app_state_mutations() {
    for action in [
        ActionKind::InputInsert,
        ActionKind::InputReplace,
        ActionKind::InputClear,
        ActionKind::InputModeSet,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateAppState,
            "{} should use MutateAppState permission",
            action.as_str()
        );
    }
}

#[test]
fn inside_warp_only_actions_reject_outside_warp_invocation_context() {
    let all_enabled = settings_with_values(true, true, true, true, true, true);

    for action in [
        ActionKind::SessionActivate,
        ActionKind::SessionPrevious,
        ActionKind::SessionNext,
        ActionKind::SessionReopen,
        ActionKind::InputInsert,
        ActionKind::InputReplace,
        ActionKind::InputClear,
        ActionKind::InputModeSet,
    ] {
        let err =
            ensure_settings_allow_action(&all_enabled, InvocationContext::OutsideWarp, action)
                .expect_err("InsideWarp-only action rejects OutsideWarp context");
        assert_eq!(
            err.code,
            ErrorCode::ExecutionContextNotAllowed,
            "{} should reject outside-Warp context",
            action.as_str()
        );
    }
}

#[test]
fn settings_mutation_actions_use_metadata_configuration_permission() {
    for action in [
        ActionKind::ThemeSet,
        ActionKind::AppearanceSet,
        ActionKind::AppearanceFontSize,
        ActionKind::AppearanceZoom,
        ActionKind::SettingSet,
        ActionKind::SettingToggle,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateMetadataConfiguration,
            "{} should use MutateMetadataConfiguration permission",
            action.as_str()
        );
    }
}

#[test]
fn settings_mutations_require_metadata_configuration_permission() {
    let metadata_config_only = settings_with_values(true, false, false, false, true, false);
    let app_state_only = settings_with_values(true, false, false, true, false, false);

    for action in [
        ActionKind::ThemeSet,
        ActionKind::AppearanceSet,
        ActionKind::SettingSet,
        ActionKind::SettingToggle,
    ] {
        ensure_settings_allow_action(
            &metadata_config_only,
            InvocationContext::OutsideWarp,
            action,
        )
        .expect("metadata config permission allows settings mutation");
        let err =
            ensure_settings_allow_action(&app_state_only, InvocationContext::OutsideWarp, action)
                .expect_err("settings mutation denied without metadata config permission");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }
}

#[test]
fn app_surface_actions_use_app_state_mutation_permission() {
    for action in [
        ActionKind::AppSettingsOpen,
        ActionKind::AppCommandPaletteOpen,
        ActionKind::AppCommandSearchOpen,
        ActionKind::AppWarpDriveOpen,
        ActionKind::AppWarpDriveToggle,
        ActionKind::AppResourceCenterToggle,
        ActionKind::AppAiAssistantToggle,
        ActionKind::AppCodeReviewToggle,
        ActionKind::AppVerticalTabsToggle,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateAppState,
            "{} should use MutateAppState permission",
            action.as_str()
        );
    }
}

#[test]
fn settings_mutations_require_authenticated_user() {
    for action in [
        ActionKind::ThemeSet,
        ActionKind::AppearanceSet,
        ActionKind::AppearanceFontSize,
        ActionKind::AppearanceZoom,
        ActionKind::SettingSet,
        ActionKind::SettingToggle,
    ] {
        assert!(
            action.metadata().requires_authenticated_user,
            "{} should require authenticated user",
            action.as_str()
        );
    }
}

#[test]
fn settings_allowlist_rejects_local_control_keys() {
    let private_err = rejected_setting_key("local_control.allow_inside_warp_control");
    assert_eq!(private_err.code, ErrorCode::NotAllowlisted);
    assert!(private_err.message.contains("private or sensitive"));

    let private_err = rejected_setting_key("local_control.allow_outside_warp_metadata_reads");
    assert_eq!(private_err.code, ErrorCode::NotAllowlisted);
    assert!(private_err.message.contains("private or sensitive"));
}

#[test]
fn action_metadata_lookup_reports_implemented_status_for_new_mutations() {
    for action_name in [
        "session.activate",
        "session.previous",
        "session.next",
        "session.reopen",
        "input.insert",
        "input.replace",
        "input.clear",
        "input.mode.set",
        "theme.set",
        "appearance.set",
        "appearance.font_size",
        "appearance.zoom",
        "setting.set",
        "setting.toggle",
        "app.settings.open",
        "app.command_palette.open",
        "app.command_search.open",
        "app.warp_drive.open",
        "app.warp_drive.toggle",
        "app.resource_center.toggle",
        "app.ai_assistant.toggle",
        "app.code_review.toggle",
        "app.vertical_tabs.toggle",
    ] {
        let metadata = action_metadata_for_name(action_name)
            .unwrap_or_else(|_| panic!("{action_name} should be allowlisted"));
        assert_eq!(
            metadata.implementation_status,
            ::local_control::ActionImplementationStatus::Implemented,
            "{action_name} should be Implemented"
        );
    }
}

#[test]
fn api_key_error_codes_serialize_stably() {
    use ::local_control::ErrorCode;
    assert_eq!(
        serde_json::to_value(ErrorCode::ApiKeyInvalid).expect("serializes"),
        serde_json::json!("api_key_invalid")
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::ApiKeyExpired).expect("serializes"),
        serde_json::json!("api_key_expired")
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::ApiKeyRevoked).expect("serializes"),
        serde_json::json!("api_key_revoked")
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::ApiKeyInsufficientScope).expect("serializes"),
        serde_json::json!("api_key_insufficient_scope")
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::ApiKeySubjectMismatch).expect("serializes"),
        serde_json::json!("api_key_subject_mismatch")
    );
}
