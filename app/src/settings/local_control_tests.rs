use std::collections::HashMap;
use std::sync::Mutex;

use super::{LocalControlMode, LocalControlModeSetting, LocalControlSettings};
use settings::{PrivatePreferences, PublicPreferences, Setting as _, SettingsManager};
use warpui::SingletonEntity as _;
use warpui_extras::secure_storage::{self, AppContextExt as _};
use warpui_extras::user_preferences;

#[derive(Default)]
struct InMemorySecureStorage {
    values: Mutex<HashMap<String, String>>,
}

impl secure_storage::SecureStorage for InMemorySecureStorage {
    fn write_value(&self, key: &str, value: &str) -> Result<(), secure_storage::Error> {
        match self.values.lock() {
            Ok(mut values) => {
                values.insert(key.to_owned(), value.to_owned());
                Ok(())
            }
            Err(err) => Err(secure_storage::Error::Unknown(anyhow::anyhow!(
                err.to_string()
            ))),
        }
    }

    fn read_value(&self, key: &str) -> Result<String, secure_storage::Error> {
        match self.values.lock() {
            Ok(values) => values
                .get(key)
                .cloned()
                .ok_or(secure_storage::Error::NotFound),
            Err(err) => Err(secure_storage::Error::Unknown(anyhow::anyhow!(
                err.to_string()
            ))),
        }
    }

    fn remove_value(&self, key: &str) -> Result<(), secure_storage::Error> {
        match self.values.lock() {
            Ok(mut values) => {
                values.remove(key);
                Ok(())
            }
            Err(err) => Err(secure_storage::Error::Unknown(anyhow::anyhow!(
                err.to_string()
            ))),
        }
    }
}

fn default_settings() -> LocalControlSettings {
    LocalControlSettings {
        local_control_mode: LocalControlModeSetting::new(None),
    }
}

#[test]
fn defaults_disable_warp_control() {
    let settings = default_settings();

    assert_eq!(LocalControlMode::default(), LocalControlMode::Disabled);
    assert_eq!(settings.mode(), LocalControlMode::Disabled);
    assert!(!settings.inside_warp_control_enabled());
    assert!(!settings.outside_warp_control_enabled());
}

#[test]
fn mode_is_persisted_to_secure_storage() {
    warpui::App::test((), |mut app| async move {
        app.update(|ctx| {
            ctx.add_singleton_model(|_| {
                PublicPreferences::new(
                    Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
                )
            });
            ctx.add_singleton_model(|_| {
                PrivatePreferences::new(
                    Box::<user_preferences::in_memory::InMemoryPreferences>::default(),
                )
            });
            ctx.add_singleton_model(|_| SettingsManager::default());
            ctx.add_singleton_model(|_| -> secure_storage::Model {
                Box::<InMemorySecureStorage>::default()
            });
            LocalControlSettings::register(ctx);
        });

        app.update(|ctx| {
            LocalControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .local_control_mode
                    .set_value(LocalControlMode::EnabledEverywhere, ctx)
            })
        })
        .expect("setting update should succeed");

        app.read(|ctx| {
            let stored = ctx
                .secure_storage()
                .read_value(LocalControlModeSetting::storage_key())
                .expect("local-control mode should be stored securely");
            let mode = serde_json::from_str::<LocalControlMode>(&stored)
                .expect("stored local-control mode should deserialize");
            assert_eq!(mode, LocalControlMode::EnabledEverywhere);

            let private_value = LocalControlModeSetting::preferences_for_setting(ctx)
                .read_value(LocalControlModeSetting::storage_key())
                .expect("private preferences should be readable");
            assert!(private_value.is_none());
        });
    });
}
