//! Interface localization for Warp.
//!
//! Warp ships no built-in i18n, so this module provides a minimal, dependency-free
//! framework: a `Language` enum, a runtime current-language cell, a `t!()` macro
//! that wraps English string literals, and a translation table per language.
//!
//! Usage: replace a hardcoded English literal `"Open"` with `t!("Open")`. The
//! macro looks up the current language and returns the translation, falling back
//! to the English literal when no translation exists — so wrapping is always
//! safe and the app never breaks on a missing key.
//!
//! Translation policy: technical terms (Warp, AI, Shell, SSH, Tab, Theme, Font,
//! Terminal, Block, Drive, Prompt, Cursor) stay in English; sentence-like copy
//! is translated naturally.

use std::collections::HashMap;
use std::sync::OnceLock;

use lazy_static::lazy_static;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use settings_value::SettingsValue;

/// Interface language for Warp's UI.
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Default,
    schemars::JsonSchema,
    SettingsValue,
)]
#[schemars(description = "Interface language.", rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    SimplifiedChinese,
}

impl Language {
    /// Human-readable name shown in the language picker itself (always in the
    /// language's own script so users can find it).
    pub fn display_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::SimplifiedChinese => "简体中文",
        }
    }

    /// All selectable languages, in display order.
    pub const ALL: [Language; 2] = [Language::English, Language::SimplifiedChinese];
}

static CURRENT: OnceLock<RwLock<Language>> = OnceLock::new();

fn current_lock() -> &'static RwLock<Language> {
    CURRENT.get_or_init(|| RwLock::new(Language::default()))
}

/// The currently active UI language. Defaults to English until `set_current`
/// is called at startup from the persisted `LocalizationSettings`.
pub fn current() -> Language {
    *current_lock().read()
}

/// Set the active UI language. Called at startup (after settings load) and
/// whenever the user changes the language setting in Appearance.
pub fn set_current(lang: Language) {
    *current_lock().write() = lang;
}

/// Translate an English string literal to the current UI language.
///
/// Returns the key unchanged for English, or when no Chinese translation is
/// registered. The caller may therefore wrap any English literal without
/// worrying about completeness.
pub fn translate(key: &'static str) -> &'static str {
    match current() {
        Language::English => key,
        Language::SimplifiedChinese => ZH_CN.get(key).copied().unwrap_or(key),
    }
}

// Simplified Chinese translation table.
//
// Keys are the exact English string literals used at `t!()` call sites.
// Untranslated keys fall back to English via `translate`, so this map only
// needs to cover strings that have actually been wrapped.
lazy_static! {
    static ref ZH_CN: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();

        // ---- Appearance page (settings_view/appearance_page.rs) ----
        m.insert("Appearance", "外观");
        m.insert("Language", "语言");
        m.insert("Theme", "主题");
        m.insert("Themes", "主题");
        m.insert("Window", "窗口");
        m.insert("Font", "字体");
        m.insert("Cursor", "光标");
        m.insert("Input", "输入");
        m.insert("Icon", "图标");
        m.insert("Panes", "面板");
        m.insert("Text", "文本");
        m.insert("Tabs", "标签页");
        m.insert("Tab", "标签页");
        m.insert("Blocks", "块");
        m.insert("Block", "块");
        m.insert("Tools panel", "工具面板");
        m.insert("Full-screen Apps", "全屏应用");

        // ---- Menu bar top-level (app_menus.rs) ----
        m.insert("File", "文件");
        m.insert("Edit", "编辑");
        m.insert("View", "视图");
        m.insert("AI", "AI"); // widely used untranslated in Chinese
        m.insert("Drive", "Drive"); // Warp Drive feature name
        m.insert("Shell", "Shell"); // widely used untranslated
        m.insert("SSH", "SSH"); // protocol name
        m.insert("Help", "帮助");
        m.insert("New Window", "新建窗口");
        m.insert("New Tab", "新建标签页");
        m.insert("Settings", "设置");
        m.insert("Open", "打开");
        m.insert("Save", "保存");
        m.insert("Close", "关闭");
        m.insert("Quit", "退出");
        m.insert("About", "关于");
        m.insert("Hide", "隐藏");
        m.insert("Show", "显示");

        // ---- Menu item consts (app_menus.rs lines 43-59) ----
        m.insert(
            "Enable Shell Debug Mode (-x) for New Sessions",
            "为新会话启用 Shell 调试模式 (-x)",
        );
        m.insert(
            "Disable Shell Debug Mode (-x) for New Sessions",
            "为新会话禁用 Shell 调试模式 (-x)",
        );
        m.insert(
            "Enable In-band Generators for New Sessions",
            "为新会话启用 In-band Generators",
        );
        m.insert(
            "Disable in-band generators for new sessions",
            "为新会话禁用 In-band Generators",
        );
        m.insert(
            "Enable PTY Recording Mode (warp.pty.recording)",
            "启用 PTY 录制模式 (warp.pty.recording)",
        );
        m.insert(
            "Disable PTY Recording Mode (warp.pty.recording)",
            "禁用 PTY 录制模式 (warp.pty.recording)",
        );
        m.insert("Show Initialization Block", "显示初始化 Block");
        m.insert("Hide Initialization Block", "隐藏初始化 Block");
        m.insert("Show In-band Command Blocks", "显示 In-band Command Blocks");
        m.insert("Hide In-band Command Blocks", "隐藏 In-band Command Blocks");
        m.insert("Show Warpified SSH Blocks", "显示 Warpified SSH Blocks");
        m.insert("Hide Warpified SSH Blocks", "隐藏 Warpified SSH Blocks");
        m.insert(
            "Export Default Settings as CSV to home dir",
            "将默认设置导出为 CSV 到主目录",
        );

        m
    };
}

/// Like `translate`, but with an explicit Chinese translation supplied inline.
/// Used by the two-argument `t!("English", "中文")` form so call sites can
/// carry their own translation without a central table entry.
pub fn translate_pair(en: &'static str, zh: &'static str) -> &'static str {
    match current() {
        Language::English => en,
        Language::SimplifiedChinese => zh,
    }
}

/// Translate a string literal to the current UI language.
///
/// # Example
/// ```ignore
/// use warp_core::t;
/// let label = t!("Open"); // "打开" when Chinese, "Open" when English
/// ```
///
/// Falls back to the English literal when no translation exists, so wrapping
/// is always safe. Only accepts string literals (`&'static str`); for dynamic
/// strings, call [`translate`] directly or keep them English.
///
/// Two-argument form `t!("English", "中文")` carries the translation inline
/// (preferred for bulk translation — no central table entry needed, and the
/// diff stays on a single line for easier upstream merges).
#[macro_export]
macro_rules! t {
    ($en:literal, $zh:literal) => {
        $crate::localization::translate_pair($en, $zh)
    };
    ($key:literal) => {
        $crate::localization::translate($key)
    };
}
