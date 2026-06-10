use local_control::protocol::{ActionKind, ThemeStateResult};

use super::{finish_session, init_session, run_stop_steps};
use crate::local_control::tour::state::TourStop;
use crate::local_control::tour::test_support::ScriptedInvoker;

#[test]
fn finish_session_restores_theme_and_closes_only_given_targets() {
    let invoker = ScriptedInvoker::default();
    let theme = ThemeStateResult {
        name: "Dracula".to_owned(),
        follow_system_theme: false,
        light_theme: Some("Light Owl".to_owned()),
        dark_theme: Some("Dracula".to_owned()),
    };
    let tabs = vec!["tab-agent".to_owned()];
    let result = finish_session(&invoker, Some("p2"), &tabs, Some(&theme));
    assert!(!result.copy.is_empty());
    assert!(
        result.steps.iter().all(|step| step.ok),
        "{:?}",
        result.steps
    );
    assert_eq!(
        invoker.actions(),
        vec![
            ActionKind::ThemeSystemSet,
            ActionKind::ThemeLightSet,
            ActionKind::ThemeDarkSet,
            ActionKind::ThemeSet,
            ActionKind::PaneClose,
            ActionKind::TabClose,
        ]
    );
    let calls = invoker.calls.borrow();
    let close = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::PaneClose)
        .expect("pane.close should be dispatched");
    assert_eq!(
        ScriptedInvoker::pane_id_for(&close.2).as_deref(),
        Some("p2")
    );
}

#[test]
fn finish_session_without_targets_or_theme_does_nothing() {
    let invoker = ScriptedInvoker::default();
    let result = finish_session(&invoker, None, &[], None);
    assert!(result.steps.is_empty());
    assert!(invoker.actions().is_empty());
}

#[test]
fn run_stop_steps_opens_surfaces_and_refocuses_anchor() {
    let invoker = ScriptedInvoker::default();
    let result = run_stop_steps(&invoker, TourStop::GlobalSearch, "p2", "p1");
    assert_eq!(result.stop, "global-search");
    assert!(!result.copy.is_empty());
    assert!(result.anchor_refocused);
    assert!(
        result.steps.iter().all(|step| step.ok),
        "{:?}",
        result.steps
    );
    assert_eq!(
        result
            .keybindings
            .iter()
            .map(|keybinding| keybinding.name.as_str())
            .collect::<Vec<_>>(),
        vec!["open_global_search"],
        "keybindings should be filtered to the stop's needles"
    );

    let calls = invoker.calls.borrow();
    let open = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::SurfaceGlobalSearchOpen)
        .expect("surface open should be dispatched");
    assert_eq!(ScriptedInvoker::pane_id_for(&open.2).as_deref(), Some("p2"));
    let last = calls.last().expect("calls should not be empty");
    assert_eq!(last.0, ActionKind::PaneFocus);
    assert_eq!(ScriptedInvoker::pane_id_for(&last.2).as_deref(), Some("p1"));
}

#[test]
fn run_stop_steps_reports_failed_refocus() {
    let invoker = ScriptedInvoker::failing(vec![ActionKind::PaneFocus]);
    let result = run_stop_steps(&invoker, TourStop::Themes, "p2", "p1");
    assert!(!result.anchor_refocused);
    assert!(
        result
            .steps
            .iter()
            .any(|step| step.step == ActionKind::PaneFocus.as_str() && !step.ok)
    );
}

#[test]
fn init_session_reports_split_failure_without_aborting() {
    let invoker = ScriptedInvoker::failing(vec![ActionKind::PaneSplit]);
    let result = init_session(&invoker).expect("init should still return a result");
    assert_eq!(result.anchor.pane_id.as_deref(), Some("p1"));
    assert!(result.tour_pane_id.is_none());
    assert!(!result.surfaces.is_empty());
    assert!(result.theme.is_some());
    let split_step = result
        .steps
        .iter()
        .find(|step| step.step == ActionKind::PaneSplit.as_str())
        .expect("split step should be reported");
    assert!(!split_step.ok);
    assert!(split_step.error.is_some());
    assert!(
        !invoker.actions().contains(&ActionKind::PaneFocus),
        "refocus should be skipped when the split fails"
    );
}

#[test]
fn init_session_identifies_tour_pane_and_saves_state() {
    let invoker = ScriptedInvoker::default();
    let result = init_session(&invoker).expect("init should succeed");
    assert_eq!(result.anchor.pane_id.as_deref(), Some("p1"));
    assert_eq!(result.tour_pane_id.as_deref(), Some("p2"));
    assert_eq!(
        result.theme.as_ref().map(|theme| theme.name.as_str()),
        Some("Dracula")
    );
    assert!(!result.surfaces.is_empty());
    assert!(
        result.steps.iter().all(|step| step.ok),
        "{:?}",
        result.steps
    );

    let calls = invoker.calls.borrow();
    let split = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::PaneSplit)
        .expect("pane.split should be dispatched");
    assert_eq!(
        ScriptedInvoker::pane_id_for(&split.2).as_deref(),
        Some("p1")
    );
    let refocus = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::PaneFocus)
        .expect("pane.focus should be dispatched");
    assert_eq!(
        ScriptedInvoker::pane_id_for(&refocus.2).as_deref(),
        Some("p1")
    );
}
