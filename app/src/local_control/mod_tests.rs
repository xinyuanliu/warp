use ::local_control::auth::CredentialGrant;
use ::local_control::protocol::ActionKind;
use ::local_control::protocol::{
    Action, BlockGetParams, BlockListParams, ControlResponse, DriveGetParams, DriveGetResult,
    DriveListParams, DriveListResult, DriveObjectType, FileTarget, PaneSelector, PaneTarget,
    SessionSelector, SessionTarget, TabSelector, TabTarget, TargetSelector, WindowSelector,
    WindowTarget,
};
use ::local_control::{ErrorCode, InstanceId, InvocationContext, RequestEnvelope};
use chrono::Duration;
use settings::Setting as _;
use warp_core::features::FeatureFlag;
use warp_core::session_id::SessionId;
use warpui::{App, SingletonEntity};

use super::{
    action_metadata_for_name, appearance_state_result, authenticated_user_subject_for_action,
    block_get_result_from_model, block_list_result_from_model, capabilities,
    ensure_feature_enabled, ensure_settings_allow_action, outside_warp_action_enabled_for_settings,
    rejected_setting_key, require_active_window_id, require_active_window_id_for_action,
    setting_get_result, setting_list_result, theme_list_result, validate_action_params,
    validate_block_get_target, validate_block_list_target, validate_drive_target,
    validate_instance_metadata_read_target, validate_tab_create_target,
    validate_terminal_read_target, LocalControlBridge,
};
use crate::auth::AuthStateProvider;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::Owner;
use crate::drive::folders::{CloudFolder, CloudFolderModel};
use crate::notebooks::{CloudNotebook, CloudNotebookModel};
use crate::server::ids::{ClientId, SyncId};
use crate::settings::{
    AllowInsideWarpControl, AllowInsideWarpReadOnly, AllowInsideWarpReadWrite,
    AllowOutsideWarpControl, AllowOutsideWarpReadOnly, AllowOutsideWarpReadWrite,
    LocalControlSettings,
};
use crate::terminal::model::TerminalModel;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workflows::{workflow::Workflow, CloudWorkflow, CloudWorkflowModel};
use crate::workspaces::user_workspaces::UserWorkspaces;

fn settings_with_values(
    inside_enabled: bool,
    outside_enabled: bool,
    inside_read_only: bool,
    outside_read_only: bool,
    inside_read_write: bool,
    outside_read_write: bool,
) -> LocalControlSettings {
    LocalControlSettings {
        allow_inside_warp_control: AllowInsideWarpControl::new(Some(inside_enabled)),
        allow_outside_warp_control: AllowOutsideWarpControl::new(Some(outside_enabled)),
        allow_inside_warp_read_only: AllowInsideWarpReadOnly::new(Some(inside_read_only)),
        allow_outside_warp_read_only: AllowOutsideWarpReadOnly::new(Some(outside_read_only)),
        allow_inside_warp_read_write: AllowInsideWarpReadWrite::new(Some(inside_read_write)),
        allow_outside_warp_read_write: AllowOutsideWarpReadWrite::new(Some(outside_read_write)),
    }
}

fn settings_with_outside_warp(
    outside_control: bool,
    outside_read_write: bool,
) -> LocalControlSettings {
    settings_with_values(true, outside_control, true, false, true, outside_read_write)
}

fn settings_with_outside_warp_read_only(
    outside_control: bool,
    outside_read_only: bool,
) -> LocalControlSettings {
    settings_with_values(true, outside_control, true, outside_read_only, true, false)
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
        InvocationContext::InsideWarp,
        Duration::minutes(5),
    );
    grant.authenticated_user.subject = authenticated_user_subject_for_action(action, ctx)
        .expect("authenticated subject check succeeds");
    grant
}

fn spoofed_authenticated_grant(action: ActionKind) -> CredentialGrant {
    let mut grant = CredentialGrant::new(
        InstanceId("inst_test".to_owned()),
        action,
        InvocationContext::InsideWarp,
        Duration::minutes(5),
    );
    grant.authenticated_user.subject = Some("spoofed-user".to_owned());
    grant
}

fn response_error_code(response: ::local_control::ResponseEnvelope) -> ErrorCode {
    let ControlResponse::Error { error } = response.response else {
        panic!("expected error response");
    };
    error.code
}

fn with_local_control_bridge(
    test: impl FnOnce(&mut LocalControlBridge, &mut warpui::ModelContext<LocalControlBridge>) + 'static,
) {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.add_singleton_model(LocalControlBridge::new);
        LocalControlBridge::handle(&app).update(&mut app, test);
    });
}

#[test]
fn tab_create_accepts_default_and_active_targets() {
    validate_tab_create_target(&TargetSelector::default()).expect("default target is accepted");

    validate_tab_create_target(&TargetSelector {
        window: Some(WindowTarget::Active),
        tab: Some(TabTarget::Active),
        pane: Some(PaneTarget::Active),
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
        tab: None,
        pane: None,
        ..TargetSelector::default()
    })
    .expect_err("concrete window target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_tab_create_target(&TargetSelector {
        window: None,
        tab: Some(TabTarget::Id {
            id: TabSelector("tab".to_owned()),
        }),
        pane: None,
        ..TargetSelector::default()
    })
    .expect_err("concrete tab target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_tab_create_target(&TargetSelector {
        window: None,
        tab: None,
        pane: Some(PaneTarget::Id {
            id: PaneSelector("pane".to_owned()),
        }),
        ..TargetSelector::default()
    })
    .expect_err("concrete pane target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);
}

#[test]
fn tab_create_rejects_unsupported_selector_forms() {
    let err = validate_tab_create_target(&TargetSelector {
        window: Some(WindowTarget::Index { index: 0 }),
        tab: None,
        pane: None,
        ..TargetSelector::default()
    })
    .expect_err("indexed window target is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    let err = validate_tab_create_target(&TargetSelector {
        window: None,
        tab: Some(TabTarget::Index { index: 0 }),
        pane: None,
        ..TargetSelector::default()
    })
    .expect_err("indexed tab target is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn capabilities_advertises_only_first_slice_core_actions() {
    assert_eq!(
        capabilities(),
        vec![
            ActionKind::InstanceList,
            ActionKind::AppPing,
            ActionKind::AppInspect,
            ActionKind::AppVersion,
            ActionKind::ActionList,
            ActionKind::ActionGet,
            ActionKind::TabCreate,
            ActionKind::BlockList,
            ActionKind::BlockGet,
            ActionKind::InputGet,
            ActionKind::HistoryList,
            ActionKind::ThemeList,
            ActionKind::AppearanceGet,
            ActionKind::SettingGet,
            ActionKind::SettingList,
            ActionKind::FileList,
            ActionKind::ProjectActive,
            ActionKind::ProjectList,
            ActionKind::DriveList,
            ActionKind::DriveGet,
        ]
    );
}

#[test]
fn terminal_reads_accept_default_and_active_targets() {
    for action in [ActionKind::InputGet, ActionKind::HistoryList] {
        validate_terminal_read_target(action, &TargetSelector::default())
            .expect("default target is accepted");

        validate_terminal_read_target(
            action,
            &TargetSelector {
                window: Some(WindowTarget::Active),
                tab: Some(TabTarget::Active),
                pane: Some(PaneTarget::Active),
                session: Some(SessionTarget::Active),
                ..TargetSelector::default()
            },
        )
        .expect("active target is accepted");
    }
}

#[test]
fn terminal_reads_reject_stale_concrete_targets() {
    let err = validate_terminal_read_target(
        ActionKind::InputGet,
        &TargetSelector {
            window: Some(WindowTarget::Id {
                id: WindowSelector("window".to_owned()),
            }),
            ..TargetSelector::default()
        },
    )
    .expect_err("concrete window target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_terminal_read_target(
        ActionKind::InputGet,
        &TargetSelector {
            pane: Some(PaneTarget::Id {
                id: PaneSelector("pane".to_owned()),
            }),
            ..TargetSelector::default()
        },
    )
    .expect_err("concrete pane target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);

    let err = validate_terminal_read_target(
        ActionKind::HistoryList,
        &TargetSelector {
            session: Some(SessionTarget::Id {
                id: SessionSelector("session".to_owned()),
            }),
            ..TargetSelector::default()
        },
    )
    .expect_err("concrete session target is rejected");
    assert_eq!(err.code, ErrorCode::StaleTarget);
}

#[test]
fn terminal_reads_reject_unsupported_selector_forms() {
    let err = validate_terminal_read_target(
        ActionKind::InputGet,
        &TargetSelector {
            tab: Some(TabTarget::Index { index: 0 }),
            ..TargetSelector::default()
        },
    )
    .expect_err("indexed tab target is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    let err = validate_terminal_read_target(
        ActionKind::HistoryList,
        &TargetSelector {
            file: Some(FileTarget::Path {
                path: "../secret".to_owned(),
            }),
            ..TargetSelector::default()
        },
    )
    .expect_err("file target is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn file_and_project_metadata_reads_reject_target_selectors() {
    validate_instance_metadata_read_target(ActionKind::FileList, &TargetSelector::default())
        .expect("default target is accepted");

    let err = validate_instance_metadata_read_target(
        ActionKind::FileList,
        &TargetSelector {
            file: Some(FileTarget::Path {
                path: "../secret".to_owned(),
            }),
            ..TargetSelector::default()
        },
    )
    .expect_err("file path selector is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);

    let err = validate_instance_metadata_read_target(
        ActionKind::ProjectList,
        &TargetSelector {
            window: Some(WindowTarget::Active),
            ..TargetSelector::default()
        },
    )
    .expect_err("project target selector is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
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
fn terminal_reads_require_active_window_with_action_specific_error() {
    let active = warpui::WindowId::from_usize(1);

    assert_eq!(
        require_active_window_id_for_action(Some(active), ActionKind::InputGet).expect("active"),
        active
    );
    let err = require_active_window_id_for_action(None, ActionKind::HistoryList)
        .expect_err("missing active window");
    assert_eq!(err.code, ErrorCode::MissingTarget);
    assert!(err.message.contains("history.list"));
}

#[test]
fn feature_flag_disabled_denies_local_control() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(false);
    let err = ensure_feature_enabled().expect_err("feature flag disabled");
    assert_eq!(err.code, ErrorCode::LocalControlDisabled);
}

#[test]
fn disabled_context_denies_before_granular_permission() {
    let settings = settings_with_values(false, true, true, true, true, true);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::InsideWarp,
        ActionKind::TabCreate,
    )
    .expect_err("inside-Warp parent context is disabled");
    assert_eq!(err.code, ErrorCode::LocalControlDisabled);
}

#[test]
fn disabled_granular_permission_denies_with_insufficient_permissions() {
    let settings = settings_with_values(true, true, true, true, false, true);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::InsideWarp,
        ActionKind::TabCreate,
    )
    .expect_err("read-write permission is disabled");
    assert_eq!(err.code, ErrorCode::InsufficientPermissions);
}

#[test]
fn metadata_read_actions_require_read_permission() {
    let settings = settings_with_values(true, true, false, true, true, true);

    for action in [
        ActionKind::ActionList,
        ActionKind::ThemeList,
        ActionKind::AppearanceGet,
        ActionKind::SettingGet,
        ActionKind::SettingList,
        ActionKind::FileList,
        ActionKind::ProjectActive,
        ActionKind::ProjectList,
        ActionKind::DriveList,
    ] {
        let err = ensure_settings_allow_action(&settings, InvocationContext::InsideWarp, action)
            .expect_err("read permission is disabled");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }
}

#[test]
fn underlying_data_read_actions_require_read_permission() {
    let settings = settings_with_values(true, true, false, true, true, true);

    for action in [ActionKind::InputGet, ActionKind::HistoryList] {
        let err = ensure_settings_allow_action(&settings, InvocationContext::InsideWarp, action)
            .expect_err("read permission is disabled");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }
}

#[test]
fn metadata_scoped_credential_cannot_invoke_input_or_history_reads() {
    let grant = CredentialGrant::new(
        InstanceId("instance".to_owned()),
        ActionKind::ActionList,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );

    for action in [ActionKind::InputGet, ActionKind::HistoryList] {
        let err = grant
            .verify_for_action(action)
            .expect_err("metadata-scoped credential cannot read underlying data");
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);
    }
}

#[test]
fn metadata_reads_require_read_only_permission() {
    let settings = settings_with_outside_warp_read_only(true, false);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::OutsideWarp,
        ActionKind::SettingGet,
    )
    .expect_err("read-only permission is disabled");
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
fn action_metadata_lookup_reports_stub_status_for_allowlisted_future_actions() {
    let metadata = action_metadata_for_name("window.list").expect("allowlisted action");

    assert_eq!(metadata.kind, ActionKind::WindowList);
    assert_eq!(
        metadata.implementation_status,
        ::local_control::ActionImplementationStatus::Stub
    );
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
fn action_list_rejects_malformed_params() {
    let err = validate_action_params(&Action {
        kind: ActionKind::ActionList,
        params: serde_json::json!({ "all": true }),
    })
    .expect_err("action.list params must be empty");
    assert_eq!(err.code, ErrorCode::InvalidParams);
}

#[test]
fn input_and_history_reject_malformed_params() {
    let err = validate_action_params(&Action {
        kind: ActionKind::InputGet,
        params: serde_json::json!({ "text": true }),
    })
    .expect_err("input.get params must be empty");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    validate_action_params(&Action {
        kind: ActionKind::InputGet,
        params: serde_json::json!({}),
    })
    .expect("empty input.get params are accepted");

    validate_action_params(&Action {
        kind: ActionKind::HistoryList,
        params: serde_json::json!({ "limit": 5 }),
    })
    .expect("history.list limit is accepted");

    let err = validate_action_params(&Action {
        kind: ActionKind::HistoryList,
        params: serde_json::json!({ "command": true }),
    })
    .expect_err("unexpected history.list params are rejected");
    assert_eq!(err.code, ErrorCode::InvalidParams);
}

#[test]
fn file_and_project_metadata_reads_reject_malformed_params() {
    for action in [
        ActionKind::FileList,
        ActionKind::ProjectActive,
        ActionKind::ProjectList,
    ] {
        let err = validate_action_params(&Action {
            kind: action,
            params: serde_json::json!({ "unexpected": true }),
        })
        .expect_err("metadata read params must be empty");
        assert_eq!(err.code, ErrorCode::InvalidParams);

        validate_action_params(&Action {
            kind: action,
            params: serde_json::json!({}),
        })
        .expect("empty metadata read params are accepted");
    }
}

#[test]
fn block_reads_require_underlying_data_permission() {
    let settings = settings_with_values(true, true, false, false, true, true);

    let err = ensure_settings_allow_action(
        &settings,
        InvocationContext::InsideWarp,
        ActionKind::BlockList,
    )
    .expect_err("underlying data read permission is disabled");
    assert_eq!(err.code, ErrorCode::InsufficientPermissions);
}

#[test]
fn metadata_read_grant_cannot_read_blocks() {
    let grant = CredentialGrant::new(
        InstanceId("instance".to_owned()),
        ActionKind::AppPing,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );

    let err = grant
        .verify_for_action(ActionKind::BlockList)
        .expect_err("metadata credential cannot read terminal data");
    assert_eq!(err.code, ErrorCode::InsufficientPermissions);
}

#[test]
fn block_read_grant_requires_authenticated_user_subject() {
    let grant = CredentialGrant::new(
        InstanceId("instance".to_owned()),
        ActionKind::BlockGet,
        InvocationContext::OutsideWarp,
        Duration::minutes(5),
    );

    let err = grant
        .verify_for_action(ActionKind::BlockGet)
        .expect_err("block.get requires authenticated user grant");
    assert_eq!(err.code, ErrorCode::AuthenticatedUserRequired);
}

#[test]
fn block_read_targets_accept_default_and_active_session() {
    validate_block_list_target(&TargetSelector::default()).expect("default target is accepted");
    validate_block_get_target(&TargetSelector {
        session: Some(SessionTarget::Active),
        ..TargetSelector::default()
    })
    .expect("active session target is accepted");
}

#[test]
fn block_list_rejects_block_selector() {
    let err = validate_block_list_target(&TargetSelector {
        block: Some(::local_control::protocol::BlockTarget::Id {
            id: ::local_control::protocol::BlockSelector("block".to_owned()),
        }),
        ..TargetSelector::default()
    })
    .expect_err("block.list does not accept block selectors");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn block_read_rejects_stale_session_targets() {
    let model = TerminalModel::mock(None, None);

    let err = block_list_result_from_model(
        &model,
        SessionId::from(42),
        true,
        BlockListParams::default(),
    )
    .expect_err("explicit session id is stale");
    assert_eq!(err.code, ErrorCode::StaleTarget);
}

#[test]
fn block_get_rejects_stale_block_targets() {
    let model = TerminalModel::mock(None, None);

    let err = block_get_result_from_model(&model, SessionId::from(0), "missing-block")
        .expect_err("block id is stale");
    assert_eq!(err.code, ErrorCode::StaleTarget);
}

#[test]
fn block_list_and_get_return_active_session_block_output() {
    let mut model = TerminalModel::mock(None, None);
    model.simulate_block("echo hi", "hello from block");
    let session_id = SessionId::from(7);
    let mut block_id = None;

    for block in model.block_list_mut().blocks_mut() {
        if block.command_to_string() == "echo hi" {
            block.set_session_id(session_id);
            block_id = Some(block.id().to_string());
        }
    }

    let Some(block_id) = block_id else {
        panic!("expected simulated block id");
    };
    let list = block_list_result_from_model(
        &model,
        session_id,
        false,
        BlockListParams { limit: Some(1) },
    )
    .expect("block list succeeds");
    assert_eq!(list.blocks.len(), 1);
    assert_eq!(list.blocks[0].block_id, block_id);
    assert_eq!(list.blocks[0].command.as_deref(), Some("echo hi"));

    let params = BlockGetParams {
        block_id: block_id.clone(),
    };
    let block = block_get_result_from_model(&model, session_id, &params.block_id)
        .expect("block get succeeds");
    assert_eq!(block.block.block_id, block_id);
    assert_eq!(block.output.as_deref(), Some("hello from block"));
}

#[test]
fn drive_actions_validate_params_and_targets() {
    validate_action_params(
        &Action::with_params(ActionKind::DriveList, DriveListParams::default())
            .expect("drive list params serialize"),
    )
    .expect("drive.list params are accepted");

    let err = validate_action_params(
        &Action::with_params(
            ActionKind::DriveGet,
            DriveGetParams {
                object_type: DriveObjectType::Workflow,
                id: String::new(),
            },
        )
        .expect("drive get params serialize"),
    )
    .expect_err("empty drive object id is rejected");
    assert_eq!(err.code, ErrorCode::InvalidParams);

    let err = validate_drive_target(
        &TargetSelector {
            window: Some(WindowTarget::Active),
            ..TargetSelector::default()
        },
        ActionKind::DriveList,
    )
    .expect_err("window selector is rejected");
    assert_eq!(err.code, ErrorCode::InvalidSelector);
}

#[test]
fn drive_list_requires_true_logged_in_user() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, false);
        let request = RequestEnvelope::new(
            Action::with_params(ActionKind::DriveList, DriveListParams::default())
                .expect("drive.list params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                request,
                spoofed_authenticated_grant(ActionKind::DriveList),
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
fn drive_list_returns_authenticated_metadata_without_content() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        create_workflow(&mut app, "build", "cargo check");
        create_notebook(&mut app, "notes", "# Notes");
        create_folder(&mut app, "folder");
        let request = RequestEnvelope::new(
            Action::with_params(ActionKind::DriveList, DriveListParams::default())
                .expect("drive.list params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                request,
                authenticated_grant(ActionKind::DriveList, ctx),
                ctx,
            );
            let ControlResponse::Ok { data } = response.response else {
                panic!("expected ok response");
            };
            let result: DriveListResult =
                serde_json::from_value(data.clone()).expect("drive list result decodes");
            assert_eq!(result.objects.len(), 2);
            assert_eq!(result.objects[0].object_type, DriveObjectType::Workflow);
            assert_eq!(result.objects[0].name, "build");
            assert_eq!(result.objects[1].object_type, DriveObjectType::Notebook);
            assert_eq!(result.objects[1].name, "notes");
            assert!(data["objects"][0].get("content").is_none());
            assert!(data["objects"][1].get("content").is_none());
        });
    })
}

#[test]
fn drive_get_returns_authenticated_underlying_content() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        let workflow_id = create_workflow(&mut app, "build", "cargo check");
        let request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveGet,
                DriveGetParams {
                    object_type: DriveObjectType::Workflow,
                    id: workflow_id,
                },
            )
            .expect("drive.get params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response =
                bridge.handle_request(request, authenticated_grant(ActionKind::DriveGet, ctx), ctx);
            let ControlResponse::Ok { data } = response.response else {
                panic!("expected ok response");
            };
            let result: DriveGetResult =
                serde_json::from_value(data).expect("drive get result decodes");
            assert_eq!(result.object.object_type, DriveObjectType::Workflow);
            assert_eq!(result.object.name, "build");
            assert_eq!(result.content["command"], "cargo check");
        });
    })
}

#[test]
fn drive_metadata_grant_cannot_read_underlying_content() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        let workflow_id = create_workflow(&mut app, "build", "cargo check");
        let request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveGet,
                DriveGetParams {
                    object_type: DriveObjectType::Workflow,
                    id: workflow_id,
                },
            )
            .expect("drive.get params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                request,
                authenticated_grant(ActionKind::DriveList, ctx),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::InsufficientPermissions
            );
        });
    })
}

#[test]
fn drive_get_rejects_unsupported_or_mismatched_objects() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_drive_app(&mut app, true);
        let folder_id = create_folder(&mut app, "folder");
        let workflow_id = create_workflow(&mut app, "build", "cargo check");
        let unsupported_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveGet,
                DriveGetParams {
                    object_type: DriveObjectType::Workflow,
                    id: folder_id,
                },
            )
            .expect("drive.get params serialize"),
        );
        let mismatched_request = RequestEnvelope::new(
            Action::with_params(
                ActionKind::DriveGet,
                DriveGetParams {
                    object_type: DriveObjectType::Notebook,
                    id: workflow_id,
                },
            )
            .expect("drive.get params serialize"),
        );
        LocalControlBridge::handle(&app).update(&mut app, |bridge, ctx| {
            let response = bridge.handle_request(
                unsupported_request,
                authenticated_grant(ActionKind::DriveGet, ctx),
                ctx,
            );
            assert_eq!(response_error_code(response), ErrorCode::UnsupportedAction);

            let response = bridge.handle_request(
                mismatched_request,
                authenticated_grant(ActionKind::DriveGet, ctx),
                ctx,
            );
            assert_eq!(
                response_error_code(response),
                ErrorCode::TargetStateConflict
            );
        });
    })
}

#[test]
fn read_only_settings_and_appearance_handlers_return_allowlisted_metadata() {
    with_local_control_bridge(|_, ctx| {
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
}

#[test]
fn setting_get_rejects_unknown_and_private_settings() {
    with_local_control_bridge(|_, ctx| {
        let err = setting_get_result("appearance.secrets.token", ctx)
            .expect_err("unknown settings are rejected");
        assert_eq!(err.code, ErrorCode::NotAllowlisted);

        let err = setting_get_result("local_control.allow_outside_warp_control", ctx)
            .expect_err("private settings are rejected");
        assert_eq!(err.code, ErrorCode::NotAllowlisted);
        assert!(err.message.contains("private or sensitive"));
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
