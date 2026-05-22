use settings::{macros::define_settings_group, SupportedPlatforms, SyncToCloud};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalControlInvocationContext {
    InsideWarp,
    OutsideWarp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalControlPermissionCategory {
    LocalMetadata,
    NonDestructiveLocalMutation,
}

define_settings_group!(LocalControlSettings, settings: [
    allow_inside_warp_control: AllowInsideWarpControl {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        private: true,
        storage_key: "LocalControlAllowInsideWarp",
        description: "Whether Warp control is allowed from verified Warp-managed terminal sessions.",
    },
    allow_outside_warp_control: AllowOutsideWarpControl {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        private: true,
        storage_key: "LocalControlAllowOutsideWarp",
        description: "Whether Warp control is allowed from external local clients.",
    },
    allow_inside_warp_local_metadata: AllowInsideWarpLocalMetadata {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        private: true,
        storage_key: "LocalControlInsideWarpLocalMetadata",
        description: "Whether verified Warp-managed terminal sessions may receive local metadata grants.",
    },
    allow_outside_warp_local_metadata: AllowOutsideWarpLocalMetadata {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        private: true,
        storage_key: "LocalControlOutsideWarpLocalMetadata",
        description: "Whether external local clients may receive local metadata grants.",
    },
    allow_inside_warp_non_destructive_mutations: AllowInsideWarpNonDestructiveMutations {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        private: true,
        storage_key: "LocalControlInsideWarpNonDestructiveMutations",
        description: "Whether verified Warp-managed terminal sessions may receive non-destructive local mutation grants.",
    },
    allow_outside_warp_non_destructive_mutations: AllowOutsideWarpNonDestructiveMutations {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        private: true,
        storage_key: "LocalControlOutsideWarpNonDestructiveMutations",
        description: "Whether external local clients may receive non-destructive local mutation grants.",
    },
]);

impl LocalControlSettings {
    pub fn is_context_enabled(&self, context: LocalControlInvocationContext) -> bool {
        match context {
            LocalControlInvocationContext::InsideWarp => *self.allow_inside_warp_control,
            LocalControlInvocationContext::OutsideWarp => *self.allow_outside_warp_control,
        }
    }

    pub fn is_permission_enabled(
        &self,
        context: LocalControlInvocationContext,
        permission: LocalControlPermissionCategory,
    ) -> bool {
        match (context, permission) {
            (
                LocalControlInvocationContext::InsideWarp,
                LocalControlPermissionCategory::LocalMetadata,
            ) => *self.allow_inside_warp_local_metadata,
            (
                LocalControlInvocationContext::OutsideWarp,
                LocalControlPermissionCategory::LocalMetadata,
            ) => *self.allow_outside_warp_local_metadata,
            (
                LocalControlInvocationContext::InsideWarp,
                LocalControlPermissionCategory::NonDestructiveLocalMutation,
            ) => *self.allow_inside_warp_non_destructive_mutations,
            (
                LocalControlInvocationContext::OutsideWarp,
                LocalControlPermissionCategory::NonDestructiveLocalMutation,
            ) => *self.allow_outside_warp_non_destructive_mutations,
        }
    }

    pub fn allows(
        &self,
        context: LocalControlInvocationContext,
        permission: LocalControlPermissionCategory,
    ) -> bool {
        self.is_context_enabled(context) && self.is_permission_enabled(context, permission)
    }
}

#[cfg(test)]
#[path = "local_control_tests.rs"]
mod tests;
