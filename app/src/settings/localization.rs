use settings::{define_settings_group, RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};
use warp_core::Language;

define_settings_group!(LocalizationSettings, settings: [
    language: LanguageSetting {
        type: Language,
        default: Language::English,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "appearance.language",
        description: "Interface language for the Warp UI.",
    },
]);
