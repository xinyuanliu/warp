use super::*;

#[test]
fn request_envelope_serializes_stable_action_names() {
    let request = RequestEnvelope::new(Action::new(ActionKind::WindowFocus));
    let value = serde_json::to_value(&request).expect("request serializes");
    assert_eq!(value["protocol_version"], PROTOCOL_VERSION);
    assert_eq!(value["action"]["kind"], "window.focus");
}

#[test]
fn read_only_metadata_actions_are_logged_out_safe_metadata_reads() {
    for action in [
        ActionKind::AppActive,
        ActionKind::WindowList,
        ActionKind::TabList,
        ActionKind::PaneList,
        ActionKind::SessionList,
        ActionKind::ThemeList,
        ActionKind::AppearanceGet,
        ActionKind::SettingGet,
        ActionKind::SettingList,
    ] {
        let metadata = action.metadata();
        assert_eq!(
            metadata.implementation_status,
            ActionImplementationStatus::Stub
        );
        assert_eq!(metadata.risk_tier, RiskTier::ReadOnlyMetadata);
        assert_eq!(
            metadata.permission_category,
            PermissionCategory::ReadMetadata
        );
        assert!(!metadata.authenticated_user.required);
        assert_eq!(
            metadata.allowed_invocation_contexts,
            vec![
                InvocationContext::InsideWarp,
                InvocationContext::OutsideWarp
            ]
        );
    }
}

#[test]
fn file_and_project_metadata_reads_are_logged_out_safe_and_implemented() {
    for action in [
        ActionKind::FileList,
        ActionKind::ProjectActive,
        ActionKind::ProjectList,
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
            vec![
                InvocationContext::InsideWarp,
                InvocationContext::OutsideWarp
            ]
        );
    }
}

#[test]
fn terminal_data_and_drive_content_reads_use_underlying_data_permission() {
    for action in [
        ActionKind::BlockList,
        ActionKind::BlockGet,
        ActionKind::InputGet,
        ActionKind::HistoryList,
        ActionKind::DriveGet,
    ] {
        let metadata = action.metadata();
        assert_eq!(
            metadata.implementation_status,
            ActionImplementationStatus::Stub
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
        assert!(metadata.authenticated_user.required);
    }
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
fn input_run_is_not_in_the_allowlisted_catalog() {
    let action = serde_json::from_value::<ActionKind>(serde_json::json!("input.run"));
    assert!(action.is_err());
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
        vec![
            InvocationContext::InsideWarp,
            InvocationContext::OutsideWarp
        ]
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
        PermissionCategory::MutateUnderlyingData
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
fn non_first_slice_actions_are_catalog_stubs() {
    let metadata = ActionKind::WindowCreate.metadata();
    assert_eq!(
        metadata.implementation_status,
        ActionImplementationStatus::Stub
    );
    assert!(
        !metadata
            .allowed_invocation_contexts
            .contains(&InvocationContext::OutsideWarp)
    );
}
