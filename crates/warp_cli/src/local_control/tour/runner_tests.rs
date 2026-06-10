use local_control::protocol::{ActionKind, TabTarget};

use super::run_tour_loop;
use crate::local_control::tour::test_support::ScriptedInvoker;

#[test]
fn agent_handoff_stages_input_without_submitting() {
    let invoker = ScriptedInvoker::default();
    let mut input: &[u8] = b"3\nHow do I split panes?\n\n4\n2\n";
    let mut output = Vec::new();
    run_tour_loop(&invoker, &mut input, &mut output, None).expect("tour should end cleanly");

    let calls = invoker.calls.borrow();
    let create = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::TabCreate)
        .expect("an agent tab should be created");
    assert_eq!(
        create.1.get("tab_type").and_then(serde_json::Value::as_str),
        Some("agent")
    );
    let insert = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::InputInsert)
        .expect("the question should be staged");
    let staged = insert
        .1
        .get("text")
        .and_then(serde_json::Value::as_str)
        .expect("staged text should be a string");
    assert!(staged.contains("How do I split panes?"));
    assert!(staged.contains("tour"));
    assert!(matches!(&insert.2.tab, Some(TabTarget::Id { id }) if id.0 == "tab-agent"));

    let text = String::from_utf8(output).expect("output should be utf-8");
    assert!(text.contains("nothing is sent (or billed) until you do"));
}

#[test]
fn themes_stop_restores_theme_once_and_cleanup_closes_tour_pane() {
    let invoker = ScriptedInvoker::default();
    let mut input: &[u8] = b"1\n1\n3\n1\n";
    let mut output = Vec::new();
    run_tour_loop(&invoker, &mut input, &mut output, None).expect("tour should end cleanly");

    let actions = invoker.actions();
    assert_eq!(
        actions
            .iter()
            .filter(|action| **action == ActionKind::ThemeSystemSet)
            .count(),
        1,
        "theme should be restored exactly once"
    );
    assert!(actions.contains(&ActionKind::SurfaceThemePickerOpen));
    assert!(actions.contains(&ActionKind::ThemeSet));
    assert!(actions.contains(&ActionKind::PaneClose));
    assert!(!actions.contains(&ActionKind::TabClose));

    let calls = invoker.calls.borrow();
    let close = calls
        .iter()
        .find(|(action, _, _)| *action == ActionKind::PaneClose)
        .expect("the tour pane should be closed");
    assert_eq!(
        ScriptedInvoker::pane_id_for(&close.2).as_deref(),
        Some("p2")
    );
}

#[test]
fn eof_triggers_best_effort_cleanup_summary() {
    let invoker = ScriptedInvoker::default();
    let mut input: &[u8] = b"";
    let mut output = Vec::new();
    run_tour_loop(&invoker, &mut input, &mut output, None).expect("eof should end gracefully");

    let actions = invoker.actions();
    assert!(!actions.contains(&ActionKind::PaneClose));
    let text = String::from_utf8(output).expect("output should be utf-8");
    assert!(text.contains("Tour ended. Still open: tour pane p2"));
}

#[test]
fn immediate_exit_leaves_everything_open() {
    let invoker = ScriptedInvoker::default();
    let mut input: &[u8] = b"4\n2\n";
    let mut output = Vec::new();
    run_tour_loop(&invoker, &mut input, &mut output, None).expect("tour should end cleanly");

    let actions = invoker.actions();
    assert!(!actions.contains(&ActionKind::PaneClose));
    assert!(!actions.contains(&ActionKind::TabClose));
    assert!(!actions.contains(&ActionKind::ThemeSet));
    assert!(!actions.contains(&ActionKind::ThemeSystemSet));

    let text = String::from_utf8(output).expect("output should be utf-8");
    assert!(text.contains("The Agentic Development Environment"));
    assert!(text.contains("Tour Complete!"));
    assert!(text.contains("I'll leave everything as is"));
}
