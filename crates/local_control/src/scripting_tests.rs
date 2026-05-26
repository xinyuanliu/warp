use chrono::Duration;

use super::*;

#[test]
fn verified_terminal_grant_carries_subject_and_scope() {
    let grant = ScriptingGrant::verified_warp_terminal(
        "session-1",
        "user-1",
        vec![ScriptingScope::ReadMetadata],
        Duration::minutes(5),
    );

    assert_eq!(grant.subject, "user-1");
    assert!(grant.has_scope(&ScriptingScope::ReadMetadata));
    assert!(!grant.has_scope(&ScriptingScope::MutateUnderlyingData));
    grant
        .verify_scope(ScriptingScope::ReadMetadata)
        .expect("scope is accepted");
}

#[test]
fn permission_categories_map_to_scripting_scopes() {
    assert_eq!(
        ScriptingScope::from_permission(PermissionCategory::ReadMetadata),
        ScriptingScope::ReadMetadata
    );
    assert_eq!(
        ScriptingScope::from_permission(PermissionCategory::MutateMetadataConfiguration),
        ScriptingScope::MutateMetadataConfiguration
    );
}
