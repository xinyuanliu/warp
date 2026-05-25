use std::fs;
use std::path::Path;

use ::local_control::auth::CredentialGrant;
use ::local_control::protocol::ActionKind;
use ::local_control::protocol::{
    Action, ControlResponse, DriveCreateParams, DriveDeleteParams, DriveInsertParams,
    DriveMutationResult, DriveObjectSelector, DriveObjectType, DriveRunParams, DriveTarget,
    DriveUpdateParams, FileDeleteParams, FileMutationResult, FileTarget, FileWriteParams,
    PaneSelector, PaneTarget, SessionSelector, SessionTarget, TabSelector, TabTarget,
    TargetSelector, WindowSelector, WindowTarget,
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
    ensure_settings_allow_action, outside_warp_action_enabled_for_settings, rejected_setting_key,
    require_active_window_id, resolve_file_mutation_path, setting_get_result, setting_list_result,
    theme_list_result, validate_action_params, validate_drive_target,
    validate_file_mutation_target, validate_tab_create_target, LocalControlBridge,
};
use crate::auth::AuthStateProvider;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::Owner;
use crate::drive::folders::{CloudFolder, CloudFolderModel};
use crate::env_vars::{
    CloudEnvVarCollection, CloudEnvVarCollectionModel, EnvVar, EnvVarCollection,
};
use crate::notebooks::{CloudNotebook, CloudNotebookModel};
use crate::projects::ProjectManagementModel;
use crate::server::ids::{ClientId, SyncId};
use crate::settings::{
    AllowOutsideWarpAppStateMutations, AllowOutsideWarpControl,
    AllowOutsideWarpMetadataConfigurationMutations, AllowOutsideWarpMetadataReads,
    AllowOutsideWarpUnderlyingDataMutations, AllowOutsideWarpUnderlyingDataReads,
    LocalControlSettings,
};
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workflows::workflow::Workflow;
use crate::workflows::{CloudWorkflow, CloudWorkflowModel};
use crate::workspaces::user_workspaces::UserWorkspaces;

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

fn enable_outside_warp_underlying_data_mutations(app: &mut App) {
    app.update(|ctx| {
        LocalControlSettings::handle(ctx).update(ctx, |settings, ctx| {
            let _ = settings.allow_outside_warp_control.set_value(true, ctx);
            let _ = settings
                .allow_outside_warp_underlying_data_mutations
                .set_value(true, ctx);
        });
    });
}

fn initialize_drive_app(app: &mut App, logged_in: bool) {
    initialize_settings_for_tests(app);
    if logged_in {
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    } else {
        app.add_singleton_model(|_| AuthStateProvider::new_logged_out_for_test());
    }
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(UserWorkspaces::default_mock);
    app.add_singleton_model(LocalControlBridge::new);
}

fn initialize_file_mutation_app(app: &mut App, root: &Path, logged_in: bool) {
    initialize_settings_for_tests(app);
    if logged_in {
        app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    } else {
        app.add_singleton_model(|_| AuthStateProvider::new_logged_out_for_test());
    }
    let root = root.to_path_buf();
    app.add_singleton_model(move |ctx| {
        let mut model = ProjectManagementModel::new(Vec::new(), None, ctx);
        model.upsert_project(root, ctx);
        model
    });
    app.add_singleton_model(LocalControlBridge::new);
}

fn create_workflow(app: &mut App, name: &str, command: &str) -> String {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        let uid = sync_id.uid();
        cloud_model.create_object(
            sync_id,
            CloudWorkflow::new_local(
                CloudWorkflowModel::new(Workflow::new(name, command)),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        uid
    })
}

fn create_notebook(app: &mut App, title: &str, data: &str) -> String {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        let uid = sync_id.uid();
        cloud_model.create_object(
            sync_id,
            CloudNotebook::new_local(
                CloudNotebookModel {
                    title: title.to_owned(),
                    data: data.to_owned(),
                    ..CloudNotebookModel::default()
                },
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        uid
    })
}

fn create_environment(app: &mut App, title: &str) -> String {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        let uid = sync_id.uid();
        cloud_model.create_object(
            sync_id,
            CloudEnvVarCollection::new_local(
                CloudEnvVarCollectionModel::new(EnvVarCollection::new(
                    Some(title.to_owned()),
                    None,
                    vec![EnvVar::new("PORT".to_owned(), "4000".to_owned(), None)],
                )),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        uid
    })
}

fn create_folder(app: &mut App, name: &str) -> String {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        let uid = sync_id.uid();
        cloud_model.create_object(
            sync_id,
            CloudFolder::new_local(
                CloudFolderModel::new(name, false),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        uid
    })
}

fn authenticated_grant(
    action: ActionKind,
    ctx: &mut warpui::ModelContext<LocalControlBridge>,
) -> CredentialGrant {
    let mut grant = CredentialGrant::new(
        InstanceId("inst_test".to_owned()),
        action,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );
    grant.authenticated_user.subject =
        super::permissions::authenticated_user_subject_for_action(action, ctx)
            .expect("authenticated subject check succeeds");
    grant
}

fn spoofed_authenticated_grant(action: ActionKind) -> CredentialGrant {
    let mut grant = CredentialGrant::new(
        InstanceId("inst_test".to_owned()),
        action,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );
    grant.authenticated_user.subject = Some("spoofed-user".to_owned());
    grant
}

fn response_drive_mutation(response: ::local_control::ResponseEnvelope) -> DriveMutationResult {
    let ControlResponse::Ok { data } = response.response else {
        panic!("expected ok response");
    };
    serde_json::from_value(data).expect("drive mutation result decodes")
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
fn capabilities_advertises_core_and_metadata_slice_actions() {
    assert_eq!(
        capabilities(),
        vec![
            ActionKind::InstanceList,
            ActionKind::AppPing,
            ActionKind::AppInspect,
            ActionKind::AppVersion,
            ActionKind::AppActive,
            ActionKind::ActionList,
            ActionKind::ActionGet,
            ActionKind::WindowList,
            ActionKind::TabList,
            ActionKind::TabCreate,
            ActionKind::PaneList,
            ActionKind::SessionList,
            ActionKind::BlockList,
            ActionKind::BlockGet,
            ActionKind::InputGet,
            ActionKind::HistoryList,
            ActionKind::ThemeList,
            ActionKind::AppearanceGet,
            ActionKind::SettingGet,
            ActionKind::SettingList,
            ActionKind::FileWrite,
            ActionKind::FileDelete,
            ActionKind::DriveCreate,
            ActionKind::DriveUpdate,
            ActionKind::DriveDelete,
            ActionKind::DriveRun,
            ActionKind::DriveInsert,
        ]
    );
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
        params: serde_json::json!({ "action": "input.run" }),
    })
    .expect_err("unallowlisted action is rejected");
    assert_eq!(err.code, ErrorCode::NotAllowlisted);
}

#[test]
fn action_metadata_lookup_reports_stub_status_for_allowlisted_future_actions() {
    let metadata = action_metadata_for_name("window.create").expect("allowlisted action");

    assert_eq!(metadata.kind, ActionKind::WindowCreate);
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
fn file_and_drive_mutations_require_underlying_data_mutation_permission() {
    let settings_no_underlying_mutation =
        settings_with_values(true, true, true, true, false, false);

    for action in [
        ActionKind::FileWrite,
        ActionKind::FileDelete,
        ActionKind::DriveCreate,
        ActionKind::DriveUpdate,
        ActionKind::DriveDelete,
        ActionKind::DriveRun,
        ActionKind::DriveInsert,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateUnderlyingData
        );
        let err = ensure_settings_allow_action(
            &settings_no_underlying_mutation,
            InvocationContext::OutsideWarp,
            action,
        )
        .expect_err("underlying data mutation permission is disabled");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }
}

#[test]
fn file_mutation_grant_requires_authenticated_user_subject() {
    let grant = CredentialGrant::new(
        InstanceId("instance".to_owned()),
        ActionKind::FileWrite,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );

    let err = grant
        .verify_for_action(ActionKind::FileWrite)
        .expect_err("file.write requires authenticated user grant");
    assert_eq!(err.code, ErrorCode::AuthenticatedUserRequired);
}

#[test]
fn drive_mutation_grant_requires_authenticated_user_subject() {
    let grant = CredentialGrant::new(
        InstanceId("instance".to_owned()),
        ActionKind::DriveCreate,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );

    let err = grant
        .verify_for_action(ActionKind::DriveCreate)
        .expect_err("drive.create requires authenticated user grant");
    assert_eq!(err.code, ErrorCode::AuthenticatedUserRequired);
}

#[test]
fn drive_target_validation_rejects_name_selectors_and_empty_ids() {
    let action = ActionKind::DriveUpdate;

    let name_target = TargetSelector {
        drive: Some(DriveTarget::Name {
            object_type: DriveObjectType::Workflow,
            name: "my-workflow".to_owned(),
        }),
        ..TargetSelector::default()
    };
    let empty_id_target = TargetSelector {
        drive: Some(DriveTarget::Id {
            object_type: DriveObjectType::Workflow,
            id: DriveObjectSelector("".to_owned()),
        }),
        ..TargetSelector::default()
    };

    let err = validate_drive_target(&name_target, action).expect_err("name selector is rejected");
    assert_eq!(err.code, ErrorCode::UnsupportedAction);

    let err =
        validate_drive_target(&empty_id_target, action).expect_err("empty id selector is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn file_mutation_target_validation_rejects_id_selectors_and_mismatched_paths() {
    let action = ActionKind::FileWrite;

    let id_target = TargetSelector {
        file: Some(FileTarget::Id {
            id: ::local_control::FileSelector("file-id".to_owned()),
        }),
        ..TargetSelector::default()
    };
    let mismatched_path_target = TargetSelector {
        file: Some(FileTarget::Path {
            path: "other.txt".to_owned(),
        }),
        ..TargetSelector::default()
    };

    let err = validate_file_mutation_target(action, &id_target, "notes.txt")
        .expect_err("file id selector is unsupported");
    assert_eq!(err.code, ErrorCode::UnsupportedAction);

    let err = validate_file_mutation_target(action, &mismatched_path_target, "notes.txt")
        .expect_err("mismatched file path target is rejected");
    assert_eq!(err.code, ErrorCode::TargetStateConflict);
}

#[test]
fn file_mutation_path_safety_rejects_traversal_and_paths_outside_roots() {
    let tempdir = tempfile::tempdir().expect("tempdir is created");
    let root = tempdir.path().join("workspace");
    let other_root = tempdir.path().join("other-workspace");
    let outside = tempdir.path().join("outside.txt");
    fs::create_dir(&root).expect("root is created");
    fs::create_dir(&other_root).expect("other root is created");
    fs::write(&outside, "outside").expect("outside file is written");
    let roots = vec![root.canonicalize().expect("root canonicalizes")];

    let err = resolve_file_mutation_path(ActionKind::FileWrite, "../secret", &roots, true)
        .expect_err("parent traversal is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    let err = resolve_file_mutation_path(
        ActionKind::FileWrite,
        &outside.display().to_string(),
        &roots,
        false,
    )
    .expect_err("absolute path outside root is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    let multiple_roots = vec![
        root.canonicalize().expect("root canonicalizes"),
        other_root.canonicalize().expect("other root canonicalizes"),
    ];
    let err =
        resolve_file_mutation_path(ActionKind::FileWrite, "relative.txt", &multiple_roots, true)
            .expect_err("relative path is ambiguous with multiple roots");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn file_mutations_require_logged_in_user() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        let tempdir = tempfile::tempdir().expect("tempdir is created");
        initialize_file_mutation_app(&mut app, tempdir.path(), false);
        enable_outside_warp_underlying_data_mutations(&mut app);
        let request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::FileWrite,
                FileWriteParams {
                    path: tempdir.path().join("new.txt").display().to_string(),
                    contents: "hello".to_owned(),
                    create: true,
                },
            )
            .expect("file.write params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                request,
                spoofed_authenticated_grant(ActionKind::FileWrite),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::AuthenticatedUserUnavailable
            );
        });
    })
}

#[test]
fn file_write_create_and_delete_succeed_for_disposable_project_files() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        let tempdir = tempfile::tempdir().expect("tempdir is created");
        initialize_file_mutation_app(&mut app, tempdir.path(), true);
        enable_outside_warp_underlying_data_mutations(&mut app);
        let existing = tempdir.path().join("existing.txt");
        fs::write(&existing, "old").expect("existing file is written");
        let created = tempdir.path().join("created.txt");

        let write_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::FileWrite,
                FileWriteParams {
                    path: existing.display().to_string(),
                    contents: "new".to_owned(),
                    create: false,
                },
            )
            .expect("file.write params serialize"),
        );
        let create_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::FileWrite,
                FileWriteParams {
                    path: created.display().to_string(),
                    contents: "created".to_owned(),
                    create: true,
                },
            )
            .expect("file.write params serialize"),
        );
        let delete_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::FileDelete,
                FileDeleteParams {
                    path: created.display().to_string(),
                    recursive: false,
                },
            )
            .expect("file.delete params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                write_request,
                authenticated_grant(ActionKind::FileWrite, ctx),
                ctx,
            );
            let ControlResponse::Ok { data } = response.response else {
                panic!("expected file.write ok response");
            };
            let result: FileMutationResult =
                serde_json::from_value(data).expect("file mutation result decodes");
            assert_eq!(result.path, existing.display().to_string());
            assert_eq!(
                fs::read_to_string(&existing).expect("existing file is read"),
                "new"
            );

            let response = bridge.handle_request(
                create_request,
                authenticated_grant(ActionKind::FileWrite, ctx),
                ctx,
            );
            let ControlResponse::Ok { data } = response.response else {
                panic!("expected file.write create ok response");
            };
            let result: FileMutationResult =
                serde_json::from_value(data).expect("file mutation result decodes");
            assert_eq!(result.path, created.display().to_string());
            assert_eq!(
                fs::read_to_string(&created).expect("created file is read"),
                "created"
            );

            let response = bridge.handle_request(
                delete_request,
                authenticated_grant(ActionKind::FileDelete, ctx),
                ctx,
            );
            let ControlResponse::Ok { data } = response.response else {
                panic!("expected file.delete ok response");
            };
            let result: FileMutationResult =
                serde_json::from_value(data).expect("file mutation result decodes");
            assert_eq!(result.path, created.display().to_string());
            assert!(!created.exists());
        });
    })
}

#[test]
fn drive_mutations_require_logged_in_user() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, false);
        enable_outside_warp_underlying_data_mutations(&mut app);
        let request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveCreate,
                DriveCreateParams {
                    object_type: DriveObjectType::Workflow,
                    name: "build".to_owned(),
                    content: serde_json::json!({ "command": "cargo check" }),
                },
            )
            .expect("drive.create params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                request,
                spoofed_authenticated_grant(ActionKind::DriveCreate),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::AuthenticatedUserUnavailable
            );
        });
    })
}

#[test]
fn drive_mutations_require_underlying_data_mutation_permission_in_bridge() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        let request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveCreate,
                DriveCreateParams {
                    object_type: DriveObjectType::Notebook,
                    name: "notes".to_owned(),
                    content: serde_json::json!({ "data": "# Notes" }),
                },
            )
            .expect("drive.create params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let grant_with_wrong_action = CredentialGrant::new(
                InstanceId("inst_test".to_owned()),
                ActionKind::AppPing,
                InvocationContext::OutsideWarp,
                Duration::minutes(5),
            );
            let response = bridge.handle_request(request, grant_with_wrong_action, ctx);
            assert_eq!(
                response_error_code(response),
                ErrorCode::InsufficientPermissions
            );
        });
    })
}

#[test]
fn drive_create_returns_safe_success_for_allowlisted_object_types() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        enable_outside_warp_underlying_data_mutations(&mut app);
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let workflow = response_drive_mutation(
                bridge.handle_request(
                    RequestEnvelope::new(
                        Action::with_params(
                            ActionKind::DriveCreate,
                            DriveCreateParams {
                                object_type: DriveObjectType::Workflow,
                                name: "build".to_owned(),
                                content: serde_json::json!({ "command": "cargo check" }),
                            },
                        )
                        .expect("drive.create workflow params serialize"),
                    ),
                    authenticated_grant(ActionKind::DriveCreate, ctx),
                    ctx,
                ),
            );
            assert_eq!(workflow.object.object_type, DriveObjectType::Workflow);
            assert_eq!(workflow.object.name, "build");

            let notebook = response_drive_mutation(
                bridge.handle_request(
                    RequestEnvelope::new(
                        Action::with_params(
                            ActionKind::DriveCreate,
                            DriveCreateParams {
                                object_type: DriveObjectType::Notebook,
                                name: "notes".to_owned(),
                                content: serde_json::json!({ "data": "# Notes" }),
                            },
                        )
                        .expect("drive.create notebook params serialize"),
                    ),
                    authenticated_grant(ActionKind::DriveCreate, ctx),
                    ctx,
                ),
            );
            assert_eq!(notebook.object.object_type, DriveObjectType::Notebook);
            assert_eq!(notebook.object.name, "notes");

            let environment = response_drive_mutation(
                bridge.handle_request(
                    RequestEnvelope::new(
                        Action::with_params(
                            ActionKind::DriveCreate,
                            DriveCreateParams {
                                object_type: DriveObjectType::Environment,
                                name: "dev".to_owned(),
                                content: serde_json::json!({
                                    "vars": [{ "name": "PORT", "value": { "Constant": "4000" } }]
                                }),
                            },
                        )
                        .expect("drive.create environment params serialize"),
                    ),
                    authenticated_grant(ActionKind::DriveCreate, ctx),
                    ctx,
                ),
            );
            assert_eq!(environment.object.object_type, DriveObjectType::Environment);
            assert_eq!(environment.object.name, "dev");
        });
    })
}

#[test]
fn drive_update_and_delete_return_safe_success() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        enable_outside_warp_underlying_data_mutations(&mut app);
        let notebook_id = create_notebook(&mut app, "notes", "# Notes");
        let environment_id = create_environment(&mut app, "dev");
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let update = response_drive_mutation(bridge.handle_request(
                RequestEnvelope::new(
                    Action::with_params(
                        ActionKind::DriveUpdate,
                        DriveUpdateParams {
                            object_type: DriveObjectType::Notebook,
                            id: notebook_id.clone(),
                            content: serde_json::json!({ "title": "updated", "data": "changed" }),
                        },
                    )
                    .expect("drive.update params serialize"),
                ),
                authenticated_grant(ActionKind::DriveUpdate, ctx),
                ctx,
            ));
            assert_eq!(update.object.name, "updated");

            let delete = response_drive_mutation(
                bridge.handle_request(
                    RequestEnvelope::new(
                        Action::with_params(
                            ActionKind::DriveDelete,
                            DriveDeleteParams {
                                object_type: DriveObjectType::Environment,
                                id: environment_id.clone(),
                            },
                        )
                        .expect("drive.delete params serialize"),
                    ),
                    authenticated_grant(ActionKind::DriveDelete, ctx),
                    ctx,
                ),
            );
            assert_eq!(delete.object.id, environment_id);
            assert_eq!(delete.object.object_type, DriveObjectType::Environment);
        });
    })
}

#[test]
fn drive_run_and_insert_fail_closed_without_policy_approval() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        enable_outside_warp_underlying_data_mutations(&mut app);
        let workflow_id = create_workflow(&mut app, "build", "cargo check");
        let notebook_id = create_notebook(&mut app, "notes", "# Notes");
        let run_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveRun,
                DriveRunParams {
                    object_type: DriveObjectType::Workflow,
                    id: workflow_id,
                },
            )
            .expect("drive.run params serialize"),
        );
        let insert_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveInsert,
                DriveInsertParams {
                    object_type: DriveObjectType::Notebook,
                    id: notebook_id,
                },
            )
            .expect("drive.insert params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                run_request,
                authenticated_grant(ActionKind::DriveRun, ctx),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::ExecutionContextNotAllowed
            );

            let response = bridge.handle_request(
                insert_request,
                authenticated_grant(ActionKind::DriveInsert, ctx),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::ExecutionContextNotAllowed
            );
        });
    })
}

#[test]
fn drive_mutations_reject_unsupported_targets_and_mismatched_types() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        enable_outside_warp_underlying_data_mutations(&mut app);
        let folder_id = create_folder(&mut app, "my-folder");
        let notebook_id = create_notebook(&mut app, "notes", "# Notes");

        let name_target_request = {
            let mut request = RequestEnvelope::new(
                Action::with_params(
                    ActionKind::DriveCreate,
                    DriveCreateParams {
                        object_type: DriveObjectType::Workflow,
                        name: "build".to_owned(),
                        content: serde_json::json!({ "command": "cargo check" }),
                    },
                )
                .expect("drive.create params serialize"),
            );
            request.target = TargetSelector {
                drive: Some(DriveTarget::Name {
                    object_type: DriveObjectType::Workflow,
                    name: "build".to_owned(),
                }),
                ..TargetSelector::default()
            };
            request
        };
        let mismatched_target_request = {
            let mut request = RequestEnvelope::new(
                Action::with_params(
                    ActionKind::DriveUpdate,
                    DriveUpdateParams {
                        object_type: DriveObjectType::Notebook,
                        id: notebook_id.clone(),
                        content: serde_json::json!({ "data": "updated" }),
                    },
                )
                .expect("drive.update params serialize"),
            );
            request.target = TargetSelector {
                drive: Some(DriveTarget::Id {
                    object_type: DriveObjectType::Workflow,
                    id: DriveObjectSelector(notebook_id),
                }),
                ..TargetSelector::default()
            };
            request
        };
        let unsupported_object_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveDelete,
                DriveDeleteParams {
                    object_type: DriveObjectType::Workflow,
                    id: folder_id,
                },
            )
            .expect("drive.delete params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                name_target_request,
                authenticated_grant(ActionKind::DriveCreate, ctx),
                ctx,
            );
            assert_eq!(response_error_code(response), ErrorCode::UnsupportedAction);

            let response = bridge.handle_request(
                mismatched_target_request,
                authenticated_grant(ActionKind::DriveUpdate, ctx),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::TargetStateConflict
            );

            let response = bridge.handle_request(
                unsupported_object_request,
                authenticated_grant(ActionKind::DriveDelete, ctx),
                ctx,
            );
            assert_eq!(response_error_code(response), ErrorCode::UnsupportedAction);
        });
    })
}
