//! Stop definitions and environment helpers for the guided Warp tour.
use std::path::Path;

use clap::ValueEnum;
use local_control::protocol::ActionKind;
use serde::Serialize;

use crate::local_control::tour::copy;

/// One surface-open step performed while demonstrating a tour stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceOpenSpec {
    pub action: ActionKind,
    pub query: Option<&'static str>,
}

impl SurfaceOpenSpec {
    const fn plain(action: ActionKind) -> Self {
        Self {
            action,
            query: None,
        }
    }

    const fn with_query(action: ActionKind, query: &'static str) -> Self {
        Self {
            action,
            query: Some(query),
        }
    }
}

/// Named tour stops accepted by `warpctrl tour stop` and the interactive runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TourStop {
    Themes,
    Keybindings,
    Panes,
    GlobalSearch,
    VerticalTabs,
    Terminal,
    Coding,
    Agents,
    Knowledge,
}

impl TourStop {
    /// Core stops, in default presentation order.
    pub(crate) const CORE: &'static [Self] = &[
        Self::Themes,
        Self::Keybindings,
        Self::Panes,
        Self::GlobalSearch,
        Self::VerticalTabs,
    ];

    /// Optional topic stops, in default presentation order.
    pub(crate) const TOPICS: &'static [Self] =
        &[Self::Terminal, Self::Coding, Self::Agents, Self::Knowledge];

    pub(crate) fn cli_name(self) -> &'static str {
        match self {
            Self::Themes => "themes",
            Self::Keybindings => "keybindings",
            Self::Panes => "panes",
            Self::GlobalSearch => "global-search",
            Self::VerticalTabs => "vertical-tabs",
            Self::Terminal => "terminal",
            Self::Coding => "coding",
            Self::Agents => "agents",
            Self::Knowledge => "knowledge",
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Themes => "Themes 🎨",
            Self::Keybindings => "Keybindings ⌨️",
            Self::Panes => "Panes & panels 🪟",
            Self::GlobalSearch => "Global search 🔍",
            Self::VerticalTabs => "Vertical tabs 📑",
            Self::Terminal => "Terminal fundamentals 🖥️",
            Self::Coding => "Coding workflow 💻",
            Self::Agents => "Agents 🤖",
            Self::Knowledge => "Knowledge & navigation 📚",
        }
    }

    pub(crate) fn copy(self) -> String {
        match self {
            Self::Themes => copy::themes(),
            Self::Keybindings => copy::keybindings(),
            Self::Panes => copy::panes(),
            Self::GlobalSearch => copy::global_search(),
            Self::VerticalTabs => copy::vertical_tabs(),
            Self::Terminal => copy::terminal(),
            Self::Coding => copy::coding(),
            Self::Agents => copy::agents(),
            Self::Knowledge => copy::knowledge(),
        }
    }

    /// Ordered surface opens demonstrated during this stop.
    pub(crate) fn surfaces(self) -> &'static [SurfaceOpenSpec] {
        const THEMES: &[SurfaceOpenSpec] =
            &[SurfaceOpenSpec::plain(ActionKind::SurfaceThemePickerOpen)];
        const KEYBINDINGS: &[SurfaceOpenSpec] =
            &[SurfaceOpenSpec::plain(ActionKind::SurfaceKeybindingsOpen)];
        const PANES: &[SurfaceOpenSpec] = &[
            SurfaceOpenSpec::plain(ActionKind::SurfaceProjectExplorerOpen),
            SurfaceOpenSpec::plain(ActionKind::SurfaceConversationListOpen),
            SurfaceOpenSpec::plain(ActionKind::SurfaceWarpDriveOpen),
            SurfaceOpenSpec::plain(ActionKind::SurfaceCodeReviewOpen),
        ];
        const GLOBAL_SEARCH: &[SurfaceOpenSpec] =
            &[SurfaceOpenSpec::plain(ActionKind::SurfaceGlobalSearchOpen)];
        const VERTICAL_TABS: &[SurfaceOpenSpec] =
            &[SurfaceOpenSpec::plain(ActionKind::SurfaceVerticalTabsOpen)];
        const TERMINAL: &[SurfaceOpenSpec] =
            &[SurfaceOpenSpec::plain(ActionKind::SurfaceCommandSearchOpen)];
        const CODING: &[SurfaceOpenSpec] = &[
            SurfaceOpenSpec::plain(ActionKind::SurfaceProjectExplorerOpen),
            SurfaceOpenSpec::plain(ActionKind::SurfaceGlobalSearchOpen),
            SurfaceOpenSpec::plain(ActionKind::SurfaceCodeReviewOpen),
        ];
        const AGENTS: &[SurfaceOpenSpec] = &[
            SurfaceOpenSpec::plain(ActionKind::SurfaceAgentManagementOpen),
            SurfaceOpenSpec::plain(ActionKind::SurfaceConversationListOpen),
            SurfaceOpenSpec::with_query(ActionKind::SurfaceSettingsOpen, "permissions"),
        ];
        const KNOWLEDGE: &[SurfaceOpenSpec] = &[
            SurfaceOpenSpec::plain(ActionKind::SurfaceWarpDriveOpen),
            SurfaceOpenSpec::with_query(ActionKind::SurfaceCommandPaletteOpen, "notebook"),
            SurfaceOpenSpec::with_query(ActionKind::SurfaceSettingsOpen, "MCP"),
        ];
        match self {
            Self::Themes => THEMES,
            Self::Keybindings => KEYBINDINGS,
            Self::Panes => PANES,
            Self::GlobalSearch => GLOBAL_SEARCH,
            Self::VerticalTabs => VERTICAL_TABS,
            Self::Terminal => TERMINAL,
            Self::Coding => CODING,
            Self::Agents => AGENTS,
            Self::Knowledge => KNOWLEDGE,
        }
    }

    /// Case-insensitive needles matched against keybinding names and
    /// descriptions to surface shortcuts relevant to this stop.
    pub(crate) fn keybinding_needles(self) -> &'static [&'static str] {
        match self {
            Self::Themes => &[],
            Self::Keybindings => &["command palette"],
            Self::Panes => &["split pane"],
            Self::GlobalSearch => &["global search"],
            Self::VerticalTabs => &["vertical tab"],
            Self::Terminal => &["command search"],
            Self::Coding => &["code review", "project explorer"],
            Self::Agents => &[],
            Self::Knowledge => &["command palette"],
        }
    }

    pub(crate) fn task(self) -> &'static str {
        match self {
            Self::Themes => {
                "Preview any theme you like in the picker — nothing applies until you click it."
            }
            Self::Keybindings => "Search the keybindings panel for a command you use every day.",
            Self::Panes => {
                "Identify the anchor pane (this one), the tour pane, and one open panel."
            }
            Self::GlobalSearch => "Search for a symbol or string you know lives in your codebase.",
            Self::VerticalTabs => "Find this tour's split pane in the vertical tabs tree.",
            Self::Terminal => {
                "Run a harmless command yourself (try `ls` or `date`), spot its block, then find it in Command Search."
            }
            Self::Coding => {
                "Check out Code Review for your current diff, or browse the Project Explorer."
            }
            Self::Agents => "Peek at Agent Management and the Permissions settings page.",
            Self::Knowledge => "Browse Warp Drive and peek at the Command Palette.",
        }
    }

    pub(crate) fn hint(self) -> &'static str {
        match self {
            Self::Themes => "The light/dark toggle lives at the top of the picker.",
            Self::Keybindings => {
                "Try typing an action name like \"new tab\" — you can also search by shortcut."
            }
            Self::Panes => "Panels slide out along the sides; panes split the tab itself.",
            Self::GlobalSearch => "Results update as you type — no Enter needed.",
            Self::VerticalTabs => "Look for the tab that holds two panes side by side.",
            Self::Terminal => "Use Command Search to filter your history as you type.",
            Self::Coding => "Code Review lives in the right panel; it tracks your repo live.",
            Self::Agents => "Agent Management is in the top-right, next to your avatar.",
            Self::Knowledge => "Warp Drive holds Workflows, Notebooks, and Rules.",
        }
    }
}

/// Maps a surface-open action to its `surface.list` destination name.
pub(crate) fn surface_name_for_action(action: ActionKind) -> Option<&'static str> {
    match action {
        ActionKind::SurfaceSettingsOpen => Some("settings"),
        ActionKind::SurfaceCommandPaletteOpen => Some("command_palette"),
        ActionKind::SurfaceCommandSearchOpen => Some("command_search"),
        ActionKind::SurfaceThemePickerOpen => Some("theme_picker"),
        ActionKind::SurfaceKeybindingsOpen => Some("keybindings"),
        ActionKind::SurfaceWarpDriveOpen => Some("warp_drive"),
        ActionKind::SurfaceCodeReviewOpen => Some("code_review"),
        ActionKind::SurfaceProjectExplorerOpen => Some("project_explorer"),
        ActionKind::SurfaceGlobalSearchOpen => Some("global_search"),
        ActionKind::SurfaceConversationListOpen => Some("conversation_list"),
        ActionKind::SurfaceVerticalTabsOpen => Some("vertical_tabs"),
        ActionKind::SurfaceAgentManagementOpen => Some("agent_management"),
        _ => None,
    }
}

/// Returns the enclosing git repository's directory name, when inside one.
pub(crate) fn repository_name(start: &Path) -> Option<String> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .and_then(|dir| dir.file_name())
        .map(|name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
