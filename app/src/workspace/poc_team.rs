use std::collections::HashMap;

use pathfinder_color::ColorU;
use warpui::{Entity, SingletonEntity, WindowId};

/// Hardcoded teams for the team-switcher POC.
///
/// In the real product these come from the user's workspace membership; here
/// they are fixed so we can demonstrate the "one window per team"
/// (Chrome-profile-style) UX without any server plumbing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PocTeam {
    Revenue,
    DevOps,
    Agents,
}

impl PocTeam {
    pub const ALL: [PocTeam; 3] = [PocTeam::Revenue, PocTeam::DevOps, PocTeam::Agents];

    pub fn display_name(self) -> &'static str {
        match self {
            PocTeam::Revenue => "Revenue",
            PocTeam::DevOps => "DevOps",
            PocTeam::Agents => "Agents",
        }
    }

    /// Accent color used to tint the window header and the switcher chip so
    /// each team's windows are visually distinguishable (like Chrome profiles).
    pub fn accent_color(self) -> ColorU {
        match self {
            PocTeam::Revenue => ColorU::new(46, 160, 67, 255),
            PocTeam::DevOps => ColorU::new(56, 139, 253, 255),
            PocTeam::Agents => ColorU::new(219, 109, 40, 255),
        }
    }
}

/// Singleton mapping each window to the team it is scoped to, mirroring the
/// per-window [`crate::workspace::WorkspaceRegistry`].
///
/// This is the client-side "scoping team per window" primitive, hardcoded for
/// the POC. Windows with no explicit assignment (e.g. the first window at
/// startup, or a plain `cmd-N` window) default to [`PocTeam::Revenue`].
pub struct PocTeamRegistry {
    teams: HashMap<WindowId, PocTeam>,
}

impl Default for PocTeamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PocTeamRegistry {
    pub fn new() -> Self {
        Self {
            teams: HashMap::new(),
        }
    }

    pub fn team_for_window(&self, window_id: WindowId) -> PocTeam {
        self.teams
            .get(&window_id)
            .copied()
            .unwrap_or(PocTeam::Revenue)
    }

    pub fn set_team(&mut self, window_id: WindowId, team: PocTeam) {
        self.teams.insert(window_id, team);
    }
}

impl Entity for PocTeamRegistry {
    type Event = ();
}

impl SingletonEntity for PocTeamRegistry {}
