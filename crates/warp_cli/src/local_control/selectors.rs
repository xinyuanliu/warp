//! CLI argument conversion into shared local-control selectors.
use local_control::protocol::{SessionSelector, SessionTarget, TargetSelector};
use local_control::selection::InstanceSelector;

use crate::local_control::TargetArgs;
pub(super) fn instance_selector(args: &TargetArgs) -> InstanceSelector {
    if let Some(instance_id) = args.instance.clone() {
        return InstanceSelector::Id(local_control::discovery::InstanceId(instance_id));
    }
    if let Some(pid) = args.pid {
        return InstanceSelector::Pid(pid);
    }
    InstanceSelector::Active
}

pub(super) fn target_selector(args: &TargetArgs) -> TargetSelector {
    TargetSelector {
        session: session_target(args),
        ..TargetSelector::default()
    }
}

fn session_target(args: &TargetArgs) -> Option<SessionTarget> {
    if let Some(session_id) = args.session_id.clone() {
        return Some(SessionTarget::Id {
            id: SessionSelector(session_id),
        });
    }
    match args.session.as_deref() {
        Some("active") => Some(SessionTarget::Active),
        Some(session_id) => Some(SessionTarget::Id {
            id: SessionSelector(session_id.to_owned()),
        }),
        None => None,
    }
}
