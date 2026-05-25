use chrono::{Duration, Utc};

use super::*;
use crate::catalog::{ActionKind, StateDataCategory};

#[test]
fn scripting_grant_scope_membership_works() {
    let grant = ScriptingGrant {
        source: ScriptingIdentitySource::ExternalApiKey {
            key_id: "key_test".to_owned(),
        },
        subject: "user@example.com".to_owned(),
        scopes: vec![ScriptingScope::LocalControlMutateUnderlyingData],
        issued_at: Utc::now(),
        expires_at: Utc::now() + Duration::minutes(5),
    };
    assert!(grant.has_scope(&ScriptingScope::LocalControlMutateUnderlyingData));
    assert!(!grant.has_scope(&ScriptingScope::LocalControlRead));
}

#[test]
fn scripting_grant_expiry_detection() {
    let expired = ScriptingGrant {
        source: ScriptingIdentitySource::ExternalApiKey {
            key_id: "key_test".to_owned(),
        },
        subject: "user@example.com".to_owned(),
        scopes: vec![],
        issued_at: Utc::now() - Duration::hours(1),
        expires_at: Utc::now() - Duration::seconds(1),
    };
    assert!(expired.is_expired());

    let valid = ScriptingGrant {
        source: ScriptingIdentitySource::ExternalApiKey {
            key_id: "key_test".to_owned(),
        },
        subject: "user@example.com".to_owned(),
        scopes: vec![],
        issued_at: Utc::now(),
        expires_at: Utc::now() + Duration::minutes(5),
    };
    assert!(!valid.is_expired());
}

#[test]
fn underlying_data_mutation_actions_require_authenticated_scripting() {
    for action in [ActionKind::InputRun] {
        assert_eq!(
            action.metadata().state_data_category,
            StateDataCategory::UnderlyingDataMutation
        );
        assert!(
            action.metadata().requires_authenticated_scripting,
            "{} should require authenticated scripting",
            action.as_str()
        );
    }
}

#[test]
fn non_underlying_mutation_actions_do_not_require_authenticated_scripting() {
    for action in [
        ActionKind::TabCreate,
        ActionKind::WindowCreate,
        ActionKind::SettingSet,
        ActionKind::ThemeSet,
        ActionKind::BlockList,
        ActionKind::HistoryList,
        ActionKind::InstanceList,
        ActionKind::InputInsert,
        ActionKind::InputReplace,
        ActionKind::InputClear,
        ActionKind::InputModeSet,
    ] {
        assert!(
            !action.metadata().requires_authenticated_scripting,
            "{} should not require authenticated scripting",
            action.as_str()
        );
    }
}

#[test]
fn api_key_storage_ref_serializes_without_raw_key() {
    let storage_ref = ApiKeyStorageRef {
        key_id: "kid_abc123".to_owned(),
        subject: "user@warp.dev".to_owned(),
        scopes: vec![
            ScriptingScope::LocalControlRead,
            ScriptingScope::LocalControlMutateUnderlyingData,
        ],
    };
    let json = serde_json::to_value(&storage_ref).expect("serializes");
    assert!(json["key_id"].as_str().is_some());
    assert!(json["subject"].as_str().is_some());
    assert!(json.get("raw_key").is_none());
    assert!(json.get("key_secret").is_none());
}

#[test]
fn scripting_identity_source_terminal_and_api_key_serialize_distinctly() {
    let terminal = ScriptingIdentitySource::VerifiedWarpTerminal {
        session_id: "sess_xyz".to_owned(),
    };
    let api_key = ScriptingIdentitySource::ExternalApiKey {
        key_id: "kid_abc".to_owned(),
    };
    let terminal_json = serde_json::to_value(&terminal).expect("serializes");
    let api_key_json = serde_json::to_value(&api_key).expect("serializes");
    assert_eq!(terminal_json["source"], "verified_warp_terminal");
    assert_eq!(api_key_json["source"], "external_api_key");
    assert!(terminal_json.get("session_id").is_some());
    assert!(api_key_json.get("key_id").is_some());
    assert!(api_key_json.get("session_id").is_none());
}
