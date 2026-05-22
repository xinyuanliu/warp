use settings::{Setting, SyncToCloud};

use super::{
    AllowInsideWarpControl, AllowInsideWarpLocalMetadata, AllowInsideWarpNonDestructiveMutations,
    AllowOutsideWarpControl, AllowOutsideWarpLocalMetadata,
    AllowOutsideWarpNonDestructiveMutations, LocalControlInvocationContext,
    LocalControlPermissionCategory, LocalControlSettings,
};

fn settings_with_values(
    inside_enabled: bool,
    outside_enabled: bool,
    inside_metadata: bool,
    outside_metadata: bool,
    inside_non_destructive: bool,
    outside_non_destructive: bool,
) -> LocalControlSettings {
    LocalControlSettings {
        allow_inside_warp_control: AllowInsideWarpControl::new(Some(inside_enabled)),
        allow_outside_warp_control: AllowOutsideWarpControl::new(Some(outside_enabled)),
        allow_inside_warp_local_metadata: AllowInsideWarpLocalMetadata::new(Some(inside_metadata)),
        allow_outside_warp_local_metadata: AllowOutsideWarpLocalMetadata::new(Some(
            outside_metadata,
        )),
        allow_inside_warp_non_destructive_mutations: AllowInsideWarpNonDestructiveMutations::new(
            Some(inside_non_destructive),
        ),
        allow_outside_warp_non_destructive_mutations: AllowOutsideWarpNonDestructiveMutations::new(
            Some(outside_non_destructive),
        ),
    }
}

#[test]
fn defaults_allow_inside_warp_tab_create_permissions_only() {
    let settings = settings_with_values(true, false, true, false, true, false);

    assert!(settings.allows(
        LocalControlInvocationContext::InsideWarp,
        LocalControlPermissionCategory::LocalMetadata
    ));
    assert!(settings.allows(
        LocalControlInvocationContext::InsideWarp,
        LocalControlPermissionCategory::NonDestructiveLocalMutation
    ));
    assert!(!settings.allows(
        LocalControlInvocationContext::OutsideWarp,
        LocalControlPermissionCategory::LocalMetadata
    ));
    assert!(!settings.allows(
        LocalControlInvocationContext::OutsideWarp,
        LocalControlPermissionCategory::NonDestructiveLocalMutation
    ));
}

#[test]
fn generated_settings_are_private_local_only_with_expected_defaults() {
    assert!(*AllowInsideWarpControl::new(None));
    assert!(!*AllowOutsideWarpControl::new(None));
    assert!(*AllowInsideWarpLocalMetadata::new(None));
    assert!(!*AllowOutsideWarpLocalMetadata::new(None));
    assert!(*AllowInsideWarpNonDestructiveMutations::new(None));
    assert!(!*AllowOutsideWarpNonDestructiveMutations::new(None));
    assert_eq!(AllowInsideWarpControl::sync_to_cloud(), SyncToCloud::Never);
    assert_eq!(AllowOutsideWarpControl::sync_to_cloud(), SyncToCloud::Never);
    assert!(AllowInsideWarpControl::is_private());
    assert!(AllowOutsideWarpControl::is_private());
}

#[test]
fn disabled_context_blocks_enabled_granular_permissions() {
    let settings = settings_with_values(false, false, true, true, true, true);

    assert!(!settings.allows(
        LocalControlInvocationContext::InsideWarp,
        LocalControlPermissionCategory::NonDestructiveLocalMutation
    ));
    assert!(!settings.allows(
        LocalControlInvocationContext::OutsideWarp,
        LocalControlPermissionCategory::LocalMetadata
    ));
}
