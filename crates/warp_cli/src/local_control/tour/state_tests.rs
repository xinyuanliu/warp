use local_control::protocol::ActionKind;

use super::{TourStop, surface_name_for_action};

const ALL_STOPS: &[TourStop] = &[
    TourStop::Themes,
    TourStop::Keybindings,
    TourStop::Panes,
    TourStop::GlobalSearch,
    TourStop::VerticalTabs,
    TourStop::Terminal,
    TourStop::Coding,
    TourStop::Agents,
    TourStop::Knowledge,
];

#[test]
fn every_stop_defines_copy_surfaces_task_and_hint() {
    for stop in ALL_STOPS {
        assert!(!stop.copy().is_empty(), "{stop:?} has no copy");
        assert!(!stop.surfaces().is_empty(), "{stop:?} has no surfaces");
        assert!(!stop.task().is_empty(), "{stop:?} has no task");
        assert!(!stop.hint().is_empty(), "{stop:?} has no hint");
        for spec in stop.surfaces() {
            assert!(
                surface_name_for_action(spec.action).is_some(),
                "{stop:?} surface action {} has no surface.list name",
                spec.action.as_str()
            );
            assert!(
                spec.action.is_implemented(),
                "{stop:?} surface action {} is not implemented",
                spec.action.as_str()
            );
        }
    }
    let core_and_topics = TourStop::CORE.len() + TourStop::TOPICS.len();
    assert_eq!(core_and_topics, ALL_STOPS.len());
    assert!(surface_name_for_action(ActionKind::TabCreate).is_none());
}
