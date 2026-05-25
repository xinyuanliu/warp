use super::*;

#[test]
fn request_envelope_serializes_stable_action_names() {
    let request = RequestEnvelope::new(Action::new(ActionKind::WindowFocus));
    let value = serde_json::to_value(&request).expect("request serializes");
    assert_eq!(value["protocol_version"], PROTOCOL_VERSION);
    assert_eq!(value["action"]["kind"], "window.focus");
}

#[test]
fn typed_params_round_trip_through_action_envelope() {
    let action = Action::with_params(
        ActionKind::SettingGet,
        SettingGetParams {
            key: "appearance.theme".to_owned(),
        },
    )
    .expect("setting params serialize");
    let params = action
        .params_as::<SettingGetParams>()
        .expect("setting params deserialize");
    assert_eq!(params.key, "appearance.theme");
}

#[test]
fn response_error_serializes_machine_code() {
    let response = ResponseEnvelope::error(
        Uuid::nil(),
        ControlError::new(ErrorCode::UnauthorizedLocalClient, "bad token"),
    );
    let value = serde_json::to_value(&response).expect("response serializes");
    assert_eq!(value["response"]["status"], "error");
    assert_eq!(
        value["response"]["error"]["code"],
        "unauthorized_local_client"
    );
}

#[test]
fn ambiguous_target_error_code_is_stable() {
    let value = serde_json::to_value(ErrorCode::AmbiguousTarget).expect("code serializes");
    assert_eq!(value, serde_json::json!("ambiguous_target"));
}

#[test]
fn malformed_action_name_is_not_deserialized() {
    let action = serde_json::from_value::<ActionKind>(serde_json::json!("tab.create.extra"));
    assert!(action.is_err());
}

#[test]
fn tab_create_metadata_is_first_slice_logged_out_safe_mutation() {
    let metadata = ActionKind::TabCreate.metadata();
    assert_eq!(
        metadata.implementation_status,
        ActionImplementationStatus::Implemented
    );
    assert_eq!(metadata.risk_tier, RiskTier::MutatingNonDestructive);
    assert_eq!(
        metadata.state_data_category,
        StateDataCategory::AppStateMutation
    );
    assert!(!metadata.requires_authenticated_user);
    assert!(!metadata.authenticated_user.required);
    assert_eq!(
        metadata.permission_category,
        PermissionCategory::MutateAppState
    );
    assert_eq!(
        metadata.allowed_invocation_contexts,
        vec![InvocationContext::OutsideWarp]
    );
}

#[test]
fn core_smoke_metadata_has_explicit_read_metadata_category() {
    for action in [
        ActionKind::InstanceList,
        ActionKind::AppPing,
        ActionKind::AppInspect,
        ActionKind::AppVersion,
        ActionKind::ActionList,
        ActionKind::ActionGet,
    ] {
        let metadata = action.metadata();
        assert_eq!(
            metadata.implementation_status,
            ActionImplementationStatus::Implemented
        );
        assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyMetadata);
        assert_eq!(
            metadata.state_data_category,
            StateDataCategory::MetadataRead
        );
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::ReadMetadata
        );
        assert!(!metadata.authenticated_user.required);
        assert!(matches!(
            metadata.target_scope,
            TargetScope::Instance | TargetScope::Action
        ));
    }
}

#[test]
fn read_only_metadata_actions_are_logged_out_safe_metadata_reads() {
    for action in [
        ActionKind::AppActive,
        ActionKind::WindowList,
        ActionKind::TabList,
        ActionKind::PaneList,
        ActionKind::SessionList,
    ] {
        let metadata = action.metadata();
        assert_eq!(
            metadata.implementation_status,
            ActionImplementationStatus::Implemented
        );
        assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyMetadata);
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::ReadMetadata
        );
        assert!(!metadata.authenticated_user.required);
        assert_eq!(
            metadata.allowed_invocation_contexts,
            vec![InvocationContext::OutsideWarp]
        );
    }
}

#[test]
fn settings_and_appearance_metadata_reads_are_implemented_logged_out_safe_reads() {
    for action in [
        ActionKind::ThemeList,
        ActionKind::AppearanceGet,
        ActionKind::SettingGet,
        ActionKind::SettingList,
    ] {
        let metadata = action.metadata();
        assert_eq!(
            metadata.implementation_status,
            ActionImplementationStatus::Implemented
        );
        assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyMetadata);
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::ReadMetadata
        );
        assert!(!metadata.authenticated_user.required);
        assert_eq!(
            metadata.allowed_invocation_contexts,
            vec![InvocationContext::OutsideWarp]
        );
    }
}

#[test]
fn underlying_data_reads_require_underlying_data_permission_and_authenticated_user() {
    for action in [
        ActionKind::BlockList,
        ActionKind::BlockGet,
        ActionKind::InputGet,
        ActionKind::HistoryList,
    ] {
        let metadata = action.metadata();
        assert_eq!(
            metadata.implementation_status,
            ActionImplementationStatus::Implemented
        );
        assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyTerminalData);
        assert_eq!(
            metadata.state_data_category,
            StateDataCategory::UnderlyingDataRead
        );
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::ReadUnderlyingData
        );
        assert!(metadata.requires_authenticated_user);
        assert!(metadata.authenticated_user.required);
    }
}

#[test]
fn underlying_data_actions_have_expected_target_scopes() {
    assert_eq!(
        ActionKind::BlockList.metadata().target_scope,
        TargetScope::Block
    );
    assert_eq!(
        ActionKind::BlockGet.metadata().target_scope,
        TargetScope::Block
    );
    assert_eq!(
        ActionKind::InputGet.metadata().target_scope,
        TargetScope::Session
    );
    assert_eq!(
        ActionKind::HistoryList.metadata().target_scope,
        TargetScope::History
    );
}

#[test]
fn action_with_params_roundtrips_typed_action_get_params() {
    let action = Action::with_params(
        ActionKind::ActionGet,
        ActionGetParams {
            action: "tab.create".to_owned(),
        },
    )
    .expect("params serialize");
    assert_eq!(action.kind, ActionKind::ActionGet);
    assert_eq!(action.params["action"], "tab.create");

    let params = action
        .params_as::<ActionGetParams>()
        .expect("params deserialize");
    assert_eq!(params.action, "tab.create");
}

#[test]
fn action_metadata_serializes_security_categories() {
    let metadata = ActionKind::TabCreate.metadata();
    let value = serde_json::to_value(metadata).expect("metadata serializes");
    assert_eq!(value["name"], "tab.create");
    assert_eq!(value["state_data_category"], "app_state_mutation");
    assert_eq!(value["permission_category"], "mutate_app_state");
    assert_eq!(
        value["authenticated_user"]["required"],
        serde_json::json!(false)
    );
}

#[test]
fn default_permissions_preserve_security_categories() {
    assert_eq!(
        ActionKind::TabCreate.metadata().permission_category,
        PermissionCategory::MutateAppState
    );
    assert_eq!(
        ActionKind::InputInsert.metadata().permission_category,
        PermissionCategory::MutateAppState
    );
    assert_eq!(
        ActionKind::TabRename.metadata().permission_category,
        PermissionCategory::MutateMetadataConfiguration
    );
    assert_eq!(
        ActionKind::SettingSet.metadata().permission_category,
        PermissionCategory::MutateMetadataConfiguration
    );
    assert_eq!(
        ActionKind::TabList.metadata().permission_category,
        PermissionCategory::ReadMetadata
    );
    assert_eq!(
        ActionKind::InputGet.metadata().permission_category,
        PermissionCategory::ReadUnderlyingData
    );
    assert_eq!(
        ActionKind::DriveGet.metadata().permission_category,
        PermissionCategory::ReadUnderlyingData
    );
}

#[test]
fn file_and_project_metadata_reads_have_correct_categories() {
    for action in [
        ActionKind::FileList,
        ActionKind::ProjectActive,
        ActionKind::ProjectList,
    ] {
        let metadata = action.metadata();
        assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyMetadata);
        assert_eq!(
            metadata.state_data_category,
            StateDataCategory::MetadataRead
        );
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::ReadMetadata
        );
        assert!(!metadata.requires_authenticated_user);
    }
}

#[test]
fn drive_content_read_is_underlying_data_permission() {
    let metadata = ActionKind::DriveGet.metadata();
    assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyTerminalData);
    assert_eq!(
        metadata.state_data_category,
        StateDataCategory::UnderlyingDataRead
    );
    assert_eq!(
        metadata.permission_category,
        PermissionCategory::ReadUnderlyingData
    );
    assert!(metadata.authenticated_user.required);
    assert_eq!(metadata.target_scope, TargetScope::Drive);
}

#[test]
fn drive_list_is_authenticated_metadata_read() {
    let metadata = ActionKind::DriveList.metadata();
    assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyMetadata);
    assert_eq!(
        metadata.permission_category,
        PermissionCategory::ReadMetadata
    );
    assert!(metadata.authenticated_user.required);
    assert_eq!(metadata.target_scope, TargetScope::Drive);
}

#[test]
fn input_run_is_underlying_data_mutation_with_authenticated_user() {
    let metadata = ActionKind::InputRun.metadata();
    assert_eq!(metadata.risk_tier, RiskTier::MutatingDestructiveOrExecution);
    assert_eq!(
        metadata.state_data_category,
        StateDataCategory::UnderlyingDataMutation
    );
    assert_eq!(
        metadata.permission_category,
        PermissionCategory::MutateUnderlyingData
    );
    assert!(metadata.authenticated_user.required);
    assert_eq!(metadata.target_scope, TargetScope::Session);
}

#[test]
fn drive_mutations_are_underlying_data_mutations_requiring_auth() {
    for action in [
        ActionKind::DriveCreate,
        ActionKind::DriveUpdate,
        ActionKind::DriveDelete,
        ActionKind::DriveRun,
        ActionKind::DriveInsert,
    ] {
        let metadata = action.metadata();
        assert_eq!(metadata.risk_tier, RiskTier::MutatingDestructiveOrExecution);
        assert_eq!(
            metadata.state_data_category,
            StateDataCategory::UnderlyingDataMutation
        );
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::MutateUnderlyingData
        );
        assert!(metadata.authenticated_user.required);
        assert_eq!(metadata.target_scope, TargetScope::Drive);
    }
}

#[test]
fn file_mutations_are_underlying_data_mutations_requiring_auth() {
    for action in [ActionKind::FileWrite, ActionKind::FileDelete] {
        let metadata = action.metadata();
        assert_eq!(metadata.risk_tier, RiskTier::MutatingDestructiveOrExecution);
        assert_eq!(
            metadata.state_data_category,
            StateDataCategory::UnderlyingDataMutation
        );
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::MutateUnderlyingData
        );
        assert!(metadata.requires_authenticated_user);
        assert!(metadata.authenticated_user.required);
        assert_eq!(metadata.target_scope, TargetScope::File);
    }
}

#[test]
fn settings_and_appearance_mutations_are_metadata_configuration_mutations() {
    for action in [
        ActionKind::ThemeSet,
        ActionKind::AppearanceSet,
        ActionKind::AppearanceFontSize,
        ActionKind::AppearanceZoom,
        ActionKind::SettingSet,
        ActionKind::SettingToggle,
    ] {
        let metadata = action.metadata();
        assert_eq!(metadata.risk_tier, RiskTier::MutatingNonDestructive);
        assert_eq!(
            metadata.state_data_category,
            StateDataCategory::MetadataConfigurationMutation
        );
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::MutateMetadataConfiguration
        );
        assert!(metadata.requires_authenticated_user);
    }
}

#[test]
fn tab_rename_is_metadata_configuration_not_app_state() {
    let metadata = ActionKind::TabRename.metadata();
    assert_eq!(metadata.risk_tier, RiskTier::MutatingNonDestructive);
    assert_eq!(
        metadata.state_data_category,
        StateDataCategory::MetadataConfigurationMutation
    );
    assert_eq!(
        metadata.permission_category,
        PermissionCategory::MutateMetadataConfiguration
    );
    assert!(metadata.requires_authenticated_user);
    assert_eq!(metadata.target_scope, TargetScope::Tab);
}

#[test]
fn file_open_is_app_state_mutation_not_underlying_data() {
    let metadata = ActionKind::FileOpen.metadata();
    assert_eq!(metadata.risk_tier, RiskTier::MutatingNonDestructive);
    assert_eq!(
        metadata.state_data_category,
        StateDataCategory::AppStateMutation
    );
    assert_eq!(
        metadata.permission_category,
        PermissionCategory::MutateAppState
    );
    assert_eq!(metadata.target_scope, TargetScope::File);
}

#[test]
fn mutating_contract_preserves_distinct_permission_categories() {
    for action in [
        ActionKind::AppFocus,
        ActionKind::AppSettingsOpen,
        ActionKind::AppCommandPaletteOpen,
        ActionKind::AppCommandSearchOpen,
        ActionKind::AppWarpDriveOpen,
        ActionKind::AppWarpDriveToggle,
        ActionKind::AppResourceCenterToggle,
        ActionKind::AppAiAssistantToggle,
        ActionKind::AppCodeReviewToggle,
        ActionKind::AppVerticalTabsToggle,
        ActionKind::WindowCreate,
        ActionKind::WindowFocus,
        ActionKind::WindowClose,
        ActionKind::TabCreate,
        ActionKind::TabActivate,
        ActionKind::TabMove,
        ActionKind::TabClose,
        ActionKind::PaneSplit,
        ActionKind::PaneFocus,
        ActionKind::PaneNavigate,
        ActionKind::PaneClose,
        ActionKind::PaneMaximize,
        ActionKind::PaneResize,
        ActionKind::PaneSessionPrevious,
        ActionKind::PaneSessionNext,
        ActionKind::SessionActivate,
        ActionKind::SessionPrevious,
        ActionKind::SessionNext,
        ActionKind::SessionReopen,
        ActionKind::InputInsert,
        ActionKind::InputReplace,
        ActionKind::InputClear,
        ActionKind::InputModeSet,
        ActionKind::FileOpen,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateAppState
        );
    }

    for action in [
        ActionKind::TabRename,
        ActionKind::ThemeSet,
        ActionKind::AppearanceSet,
        ActionKind::AppearanceFontSize,
        ActionKind::AppearanceZoom,
        ActionKind::SettingSet,
        ActionKind::SettingToggle,
    ] {
        assert_eq!(
            action.metadata().permission_category,
            PermissionCategory::MutateMetadataConfiguration
        );
    }

    for action in [
        ActionKind::InputRun,
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
    }
}
