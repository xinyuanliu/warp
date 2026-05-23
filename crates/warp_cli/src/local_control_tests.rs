use std::ffi::OsString;

use clap::Parser as _;
use clap_complete::aot::Shell;
use local_control::protocol::{ControlError, ErrorCode};
use serde_json::json;
use serial_test::serial;

use super::*;

const DISCOVERY_DIR_ENV: &str = "WARP_LOCAL_CONTROL_DISCOVERY_DIR";

fn set_discovery_dir(path: &std::path::Path) -> Option<OsString> {
    let previous = std::env::var_os(DISCOVERY_DIR_ENV);
    unsafe { std::env::set_var(DISCOVERY_DIR_ENV, path) };
    previous
}

fn restore_discovery_dir(previous: Option<OsString>) {
    match previous {
        Some(value) => unsafe { std::env::set_var(DISCOVERY_DIR_ENV, value) },
        None => unsafe { std::env::remove_var(DISCOVERY_DIR_ENV) },
    }
}
#[test]
fn parses_first_slice_tab_create() {
    let args = ControlArgs::try_parse_from(["warpctrl", "tab", "create", "--instance", "inst_123"])
        .expect("tab create parses");
    let ControlCommand::Tab(TabCommand::Create(target)) = args.command else {
        panic!("expected tab create command");
    };
    assert_eq!(target.instance.as_deref(), Some("inst_123"));
}

#[test]
fn parses_first_slice_instance_list() {
    let args = ControlArgs::try_parse_from(["warpctrl", "instance", "list"])
        .expect("instance list parses");
    assert!(matches!(
        args.command,
        ControlCommand::Instance(InstanceCommand::List)
    ));
}

#[test]
fn parses_first_slice_app_smoke_metadata_commands() {
    assert!(ControlArgs::try_parse_from(["warpctrl", "app", "ping"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "app", "version"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "app", "active"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "app", "inspect"]).is_ok());
}

#[test]
fn parses_completion_generation_command() {
    let args = ControlArgs::try_parse_from(["warpctrl", "completions", "bash"])
        .expect("completions parses");
    assert!(matches!(
        args.command,
        ControlCommand::Completions {
            shell: Some(Shell::Bash)
        }
    ));
}

#[test]
fn parses_read_only_contract_commands() {
    let commands = [
        vec!["warpctrl", "action", "list"],
        vec!["warpctrl", "action", "get", "tab.create"],
        vec!["warpctrl", "window", "list"],
        vec!["warpctrl", "tab", "list"],
        vec!["warpctrl", "pane", "list"],
        vec!["warpctrl", "session", "list"],
        vec!["warpctrl", "block", "list", "--limit", "10"],
        vec!["warpctrl", "block", "get", "block_123"],
        vec!["warpctrl", "input", "get"],
        vec!["warpctrl", "history", "list", "--limit", "20"],
        vec!["warpctrl", "theme", "list"],
        vec!["warpctrl", "appearance", "get"],
        vec!["warpctrl", "setting", "list"],
        vec!["warpctrl", "setting", "get", "appearance.theme"],
        vec!["warpctrl", "file", "list"],
        vec!["warpctrl", "project", "active"],
        vec!["warpctrl", "project", "list"],
        vec!["warpctrl", "drive", "list", "--type", "workflow"],
        vec![
            "warpctrl",
            "drive",
            "get",
            "--type",
            "notebook",
            "notebook_123",
        ],
    ];
    for command in commands {
        ControlArgs::try_parse_from(command).expect("read-only command parses");
    }
}

#[test]
fn rejects_mutating_commands_outside_contract_scope() {
    assert!(ControlArgs::try_parse_from(["warpctrl", "window", "create"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "pane", "split"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "setting", "set"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "input", "insert", "cargo check"]).is_err());
}

#[test]
fn generated_bash_completions_include_first_slice_commands() {
    let completions =
        generate_completion_string(Shell::Bash).expect("bash completions render to UTF-8");
    assert!(completions.contains("instance"));
    assert!(completions.contains("tab"));
    assert!(completions.contains("window"));
    assert!(completions.contains("setting"));
    assert!(completions.contains("project"));
    assert!(completions.contains("drive"));
    assert!(completions.contains("completions"));
}

#[test]
fn structured_error_output_uses_stable_error_code() {
    let error = ControlError::new(ErrorCode::NoInstance, "no local Warp control instances");
    let value = serde_json::to_value(ErrorSummary {
        ok: false,
        error: &error,
    })
    .expect("error summary serializes");
    assert_eq!(value["ok"], json!(false));
    assert_eq!(value["error"]["code"], json!("no_instance"));
    assert_eq!(
        value["error"]["message"],
        json!("no local Warp control instances")
    );
}

#[test]
#[serial]
fn tab_create_without_discovery_records_reports_no_instance() {
    let dir = std::env::temp_dir().join(format!(
        "warpctrl-empty-discovery-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).expect("temp discovery dir is created");
    let previous = set_discovery_dir(&dir);
    let args =
        ControlArgs::try_parse_from(["warpctrl", "--output-format", "json", "tab", "create"])
            .expect("tab create parses");
    let error = run_inner(args).expect_err("missing instance is rejected");
    restore_discovery_dir(previous);
    assert_eq!(error.code, ErrorCode::NoInstance);
}
