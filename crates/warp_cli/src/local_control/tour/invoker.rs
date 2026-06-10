//! Action invocation abstraction so tour flows can run against a live Warp
//! instance or a scripted test double.
use local_control::discovery::InstanceRecord;
use local_control::protocol::{
    ActionKind, ControlError, PaneSelector, PaneTarget, TabSelector, TabTarget, TargetSelector,
};

use crate::local_control::commands::invoke_action_on;

/// Dispatches one allowlisted action and returns its structured data payload.
pub(crate) trait ActionInvoker {
    fn invoke(
        &self,
        action: ActionKind,
        params: serde_json::Value,
        target: TargetSelector,
    ) -> Result<serde_json::Value, ControlError>;
}

/// Invoker backed by the authenticated loopback client for one selected instance.
pub(crate) struct ClientInvoker {
    instance: InstanceRecord,
}

impl ClientInvoker {
    pub(crate) fn new(instance: InstanceRecord) -> Self {
        Self { instance }
    }
}

impl ActionInvoker for ClientInvoker {
    fn invoke(
        &self,
        action: ActionKind,
        params: serde_json::Value,
        target: TargetSelector,
    ) -> Result<serde_json::Value, ControlError> {
        invoke_action_on(&self.instance, target, action, params)
    }
}

/// Targets a specific pane by opaque ID.
pub(crate) fn pane_target(pane_id: &str) -> TargetSelector {
    TargetSelector {
        pane: Some(PaneTarget::Id {
            id: PaneSelector(pane_id.to_owned()),
        }),
        ..Default::default()
    }
}

/// Targets a specific tab by opaque ID.
pub(crate) fn tab_target(tab_id: &str) -> TargetSelector {
    TargetSelector {
        tab: Some(TabTarget::Id {
            id: TabSelector(tab_id.to_owned()),
        }),
        ..Default::default()
    }
}
