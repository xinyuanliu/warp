//! Scripted [`ActionInvoker`] test double shared by tour unit tests.
use std::cell::RefCell;

use local_control::protocol::{ActionKind, ControlError, ErrorCode, TargetSelector};
use serde_json::{Value, json};

use crate::local_control::tour::invoker::ActionInvoker;

/// Records every dispatched action and serves canned responses.
pub(crate) struct ScriptedInvoker {
    pub calls: RefCell<Vec<(ActionKind, Value, TargetSelector)>>,
    pub pane_lists: RefCell<Vec<Value>>,
    pub failures: Vec<ActionKind>,
}

impl Default for ScriptedInvoker {
    fn default() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            pane_lists: RefCell::new(vec![
                json!({ "panes": [{ "pane_id": "p1" }] }),
                json!({ "panes": [{ "pane_id": "p1" }, { "pane_id": "p2" }] }),
            ]),
            failures: Vec::new(),
        }
    }
}

impl ScriptedInvoker {
    pub fn failing(failures: Vec<ActionKind>) -> Self {
        Self {
            failures,
            ..Default::default()
        }
    }

    pub fn actions(&self) -> Vec<ActionKind> {
        self.calls
            .borrow()
            .iter()
            .map(|(action, _, _)| *action)
            .collect()
    }

    pub fn pane_id_for(target: &TargetSelector) -> Option<String> {
        match &target.pane {
            Some(local_control::protocol::PaneTarget::Id { id }) => Some(id.0.clone()),
            _ => None,
        }
    }
}

impl ActionInvoker for ScriptedInvoker {
    fn invoke(
        &self,
        action: ActionKind,
        params: Value,
        target: TargetSelector,
    ) -> Result<Value, ControlError> {
        self.calls
            .borrow_mut()
            .push((action, params, target.clone()));
        if self.failures.contains(&action) {
            return Err(ControlError::new(
                ErrorCode::UnsupportedAction,
                "scripted failure",
            ));
        }
        Ok(match action {
            ActionKind::AppActive => json!({
                "action": "app.active",
                "active": {
                    "window_id": "w1",
                    "tab_id": "t1",
                    "pane_id": "p1",
                    "session_id": "s1",
                },
            }),
            ActionKind::SurfaceList => json!({
                "surfaces": [
                    { "name": "settings", "is_available": true },
                    { "name": "command_palette", "is_available": true },
                    { "name": "command_search", "is_available": true },
                    { "name": "theme_picker", "is_available": true },
                    { "name": "keybindings", "is_available": true },
                    { "name": "warp_drive", "is_available": true },
                    { "name": "code_review", "is_available": true },
                    { "name": "project_explorer", "is_available": true },
                    { "name": "global_search", "is_available": true },
                    { "name": "conversation_list", "is_available": true },
                    { "name": "vertical_tabs", "is_available": true },
                    { "name": "agent_management", "is_available": true },
                ],
            }),
            ActionKind::ThemeGet => json!({
                "name": "Dracula",
                "follow_system_theme": false,
                "light_theme": "Light Owl",
                "dark_theme": "Dracula",
            }),
            ActionKind::PaneList => {
                let mut pane_lists = self.pane_lists.borrow_mut();
                if pane_lists.is_empty() {
                    json!({ "panes": [] })
                } else {
                    pane_lists.remove(0)
                }
            }
            ActionKind::TabCreate => json!({
                "action": "tab.create",
                "tab": { "id": "tab-agent" },
            }),
            ActionKind::KeybindingList => json!({
                "keybindings": [
                    {
                        "name": "open_global_search",
                        "description": "Open Global Search",
                        "keystroke": "cmd-shift-f",
                    },
                    {
                        "name": "open_command_palette",
                        "description": "Open the Command Palette",
                        "keystroke": "cmd-p",
                    },
                ],
            }),
            _ => json!({ "action": action.as_str() }),
        })
    }
}
