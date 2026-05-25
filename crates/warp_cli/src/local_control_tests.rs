use std::ffi::OsString;

use clap::Parser as _;
use clap_complete::aot::Shell;
use local_control::protocol::{ControlError, ErrorCode, PaneTarget, TabTarget, WindowTarget};
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
    assert_eq!(target.target.instance.as_deref(), Some("inst_123"));
    assert_eq!(target.tab_type, TabType::Terminal);
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
fn parses_shared_selector_aliases() {
    let args = ControlArgs::try_parse_from([
        "warpctrl",
        "pane",
        "split",
        "--direction",
        "right",
        "--instance",
        "inst_123",
        "--window-id",
        "win_123",
        "--tab-index",
        "2",
        "--pane",
        "active",
        "--session-id",
        "sess_123",
        "--block-index",
        "4",
    ])
    .expect("pane split parses");
    let ControlCommand::Pane(PaneCommand::Split(args)) = args.command else {
        panic!("expected pane split command");
    };
    assert_eq!(args.target.instance.as_deref(), Some("inst_123"));
    assert_eq!(args.target.window_id.as_deref(), Some("win_123"));
    assert_eq!(args.target.tab_index, Some(2));
    assert_eq!(args.target.pane.as_deref(), Some("active"));
    assert_eq!(args.target.session_id.as_deref(), Some("sess_123"));
    assert_eq!(args.target.block_index, Some(4));
}

#[test]
fn rejects_conflicting_shared_selectors() {
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "tab",
            "list",
            "--tab-id",
            "tab_123",
            "--tab-index",
            "1",
        ])
        .is_err()
    );
}

#[test]
fn converts_protocol_target_selectors() {
    let args = ControlArgs::try_parse_from([
        "warpctrl",
        "tab",
        "create",
        "--window",
        "title:Build logs",
        "--tab",
        "index:3",
        "--pane-id",
        "pane_123",
    ])
    .expect("tab create parses");
    let ControlCommand::Tab(TabCommand::Create(args)) = args.command else {
        panic!("expected tab create command");
    };
    let target = selectors::target_selector(args.target).expect("selectors convert");
    assert!(matches!(
        target.window,
        Some(WindowTarget::Title { ref title }) if title == "Build logs"
    ));
    assert!(matches!(target.tab, Some(TabTarget::Index { index: 3 })));
    assert!(matches!(
        target.pane,
        Some(PaneTarget::Id { ref id }) if id.0 == "pane_123"
    ));
}

#[test]
fn parses_read_only_command_surface() {
    assert!(ControlArgs::try_parse_from(["warpctrl", "instance", "inspect"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "app", "active"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "capability", "list"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "window", "list"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "tab", "inspect", "--tab", "active"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "pane", "list", "--tab", "active"]).is_ok());
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "session", "inspect", "--session-id", "sess_1"])
            .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "block",
            "output",
            "--block-id",
            "block_1",
            "--plain"
        ])
        .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "input", "get", "--session", "active"]).is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "history", "list", "--limit", "5"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "theme", "list"]).is_ok());
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "setting",
            "get",
            "appearance.themes.system_theme"
        ])
        .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "keybinding", "get", "open_command_palette"])
            .is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "action", "inspect", "tab.create"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "file", "list"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "project", "active"]).is_ok());
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "drive", "list", "--type", "workflow"]).is_ok()
    );
}

#[test]
fn parses_mutating_command_surface_without_execution_submit() {
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "window", "create", "--shell", "zsh"]).is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "window", "close", "--window-title", "Scratch"])
            .is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "tab", "create", "--type", "agent"]).is_ok());
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "tab",
            "rename",
            "Build logs",
            "--tab-id",
            "tab_1"
        ])
        .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "tab", "color", "set", "blue", "--tab", "active"])
            .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "pane", "navigate", "--direction", "previous"])
            .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "input",
            "insert",
            "cargo check",
            "--session-id",
            "sess_1"
        ])
        .is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "input", "replace", "cargo check"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "input", "clear"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "input", "mode", "set", "agent"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "theme", "system", "set", "true"]).is_ok());
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "appearance", "font-size", "increase"]).is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "appearance", "zoom", "reset"]).is_ok());
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "setting",
            "toggle",
            "editor.syntax_highlighting"
        ])
        .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "surface",
            "settings",
            "open",
            "--page",
            "scripting"
        ])
        .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "surface",
            "command-palette",
            "open",
            "--query",
            "Settings"
        ])
        .is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "surface", "warp-drive", "toggle"]).is_ok());
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "file", "open", "src/main.rs", "--line", "10"])
            .is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "project", "open", "/tmp/project"]).is_ok());
}

#[test]
fn parses_drive_share_surface_and_native_team_share_only() {
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "drive", "object", "share", "open", "obj_1"])
            .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "drive", "object", "share-to-team", "obj_1"])
            .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "drive", "object", "share", "external", "obj_1"])
            .is_err()
    );
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "drive",
            "object",
            "public-link",
            "create",
            "obj_1"
        ])
        .is_err()
    );
}

#[test]
fn excludes_local_file_content_crud_commands() {
    assert!(ControlArgs::try_parse_from(["warpctrl", "file", "read", "src/main.rs"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "file", "create", "src/main.rs"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "file", "write", "src/main.rs"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "file", "append", "src/main.rs"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "file", "delete", "src/main.rs"]).is_err());
}

#[test]
fn excludes_command_and_agent_prompt_submission() {
    assert!(ControlArgs::try_parse_from(["warpctrl", "input", "run", "cargo check"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "agent", "prompt", "hello"]).is_err());
    assert!(ControlArgs::try_parse_from(["warpctrl", "command", "accept"]).is_err());
}

#[test]
fn parses_auth_surface_stubs() {
    assert!(ControlArgs::try_parse_from(["warpctrl", "auth", "status"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "auth", "login"]).is_ok());
    assert!(
        ControlArgs::try_parse_from([
            "warpctrl",
            "auth",
            "api-key",
            "set",
            "--key-env",
            "WARP_SCRIPTING_API_KEY"
        ])
        .is_ok()
    );
    assert!(
        ControlArgs::try_parse_from(["warpctrl", "auth", "api-key", "set", "--key-stdin"]).is_ok()
    );
    assert!(ControlArgs::try_parse_from(["warpctrl", "auth", "api-key", "status"]).is_ok());
    assert!(ControlArgs::try_parse_from(["warpctrl", "auth", "api-key", "revoke"]).is_ok());
}

#[test]
fn parses_global_output_formats() {
    let args =
        ControlArgs::try_parse_from(["warpctrl", "--output-format", "ndjson", "instance", "list"])
            .expect("ndjson output parses");
    assert_eq!(args.output_format, crate::agent::OutputFormat::Ndjson);
}

#[test]
fn generated_bash_completions_include_expanded_commands() {
    let completions =
        generate_completion_string(Shell::Bash).expect("bash completions render to UTF-8");
    assert!(completions.contains("instance"));
    assert!(completions.contains("window"));
    assert!(completions.contains("pane"));
    assert!(completions.contains("drive"));
    assert!(completions.contains("auth"));
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

#[test]
fn tab_create_options_are_parser_only_until_handler_contract_lands() {
    let args = ControlArgs::try_parse_from(["warpctrl", "tab", "create", "--type", "agent"])
        .expect("agent tab create parses");
    let error = run_inner(args).expect_err("agent tab create is a parser-only stub");
    assert_eq!(error.code, ErrorCode::UnsupportedAction);
}
