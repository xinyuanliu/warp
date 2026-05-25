//! Implementations for user-facing `warpctrl` command groups.
use local_control::protocol::{
    Action, ActionKind, ActionMetadata, ControlError, ErrorCode, RequestEnvelope,
};
use local_control::selection::select_instance;
use serde::Serialize;
use serde_json::json;

use crate::agent::OutputFormat;
use crate::local_control::output::{write_json, write_json_line};
use crate::local_control::selectors::{instance_selector, target_selector};
use crate::local_control::{
    ActionCommand, AppCommand, AppearanceCommand, AuthCommand, BlockCommand, CapabilityCommand,
    DriveCommand, FileCommand, HistoryCommand, InputCommand, InstanceCommand, KeybindingCommand,
    PaneCommand, ProjectCommand, SessionCommand, SettingCommand, SurfaceCommand, TabCommand,
    TabCreateArgs, TabType, TargetArgs, ThemeCommand, WindowCommand,
};

/// Display-oriented projection of a discoverable Warp instance.
#[derive(Serialize)]
struct InstanceSummary {
    instance_id: String,
    pid: u32,
    channel: String,
    app_id: String,
    app_version: Option<String>,
    started_at: String,
    endpoint: Option<local_control::discovery::ControlEndpoint>,
    outside_warp_control_enabled: bool,
    actions: Vec<ActionMetadata>,
}

impl From<local_control::discovery::InstanceRecord> for InstanceSummary {
    fn from(record: local_control::discovery::InstanceRecord) -> Self {
        Self {
            instance_id: record.instance_id.0,
            pid: record.pid,
            channel: record.channel,
            app_id: record.app_id,
            app_version: record.app_version,
            started_at: record.started_at.to_rfc3339(),
            endpoint: record.endpoint,
            outside_warp_control_enabled: record.outside_warp_control_enabled,
            actions: record.actions,
        }
    }
}

#[derive(Serialize)]
struct StubSummary<'a> {
    ok: bool,
    action: &'a str,
    implemented: bool,
}

pub(super) fn run_instance_command(
    command: InstanceCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        InstanceCommand::List => {
            let summaries = local_control::discovery::list_instances()
                .into_iter()
                .map(InstanceSummary::from)
                .collect::<Vec<_>>();
            write_values(summaries, output_format)
        }
        InstanceCommand::Inspect(args) => {
            let records = local_control::discovery::list_instances();
            let selector = instance_selector(args);
            let instance = select_instance(&records, &selector)?;
            let summary = InstanceSummary::from(instance.clone());
            write_value(&summary, output_format)
        }
    }
}

pub(super) fn run_app_command(
    command: AppCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        AppCommand::Ping(args) => run_action(args, ActionKind::AppPing, json!({}), output_format),
        AppCommand::Version(args) => {
            run_action(args, ActionKind::AppVersion, json!({}), output_format)
        }
        AppCommand::Active(_) => unsupported_action("app.active"),
        AppCommand::Focus(_) => unsupported_action("app.focus"),
    }
}

pub(super) fn run_capability_command(
    command: CapabilityCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        CapabilityCommand::List(_) => write_action_metadata(output_format),
        CapabilityCommand::Inspect(args) => {
            write_named_action_metadata(&args.action, output_format)
        }
    }
}

pub(super) fn run_window_command(
    command: WindowCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        WindowCommand::List(args) => {
            run_action(args, ActionKind::WindowList, json!({}), output_format)
        }
        WindowCommand::Inspect(_) => unsupported_action("window.inspect"),
        WindowCommand::Create(_) => unsupported_action("window.create"),
        WindowCommand::Focus(_) => unsupported_action("window.focus"),
        WindowCommand::Close(_) => unsupported_action("window.close"),
    }
}

pub(super) fn run_tab_command(
    command: TabCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        TabCommand::List(args) => run_action(args, ActionKind::TabList, json!({}), output_format),
        TabCommand::Inspect(_) => unsupported_action("tab.inspect"),
        TabCommand::Create(args) => run_tab_create(args, output_format),
        TabCommand::Activate(_) => unsupported_action("tab.activate"),
        TabCommand::Move(_) => unsupported_action("tab.move"),
        TabCommand::Rename(_) => unsupported_action("tab.rename"),
        TabCommand::ResetName(_) => unsupported_action("tab.reset-name"),
        TabCommand::Color(_) => unsupported_action("tab.color"),
        TabCommand::Close(_) => unsupported_action("tab.close"),
    }
}

pub(super) fn run_pane_command(
    command: PaneCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        PaneCommand::List(args) => run_action(args, ActionKind::PaneList, json!({}), output_format),
        PaneCommand::Inspect(_) => unsupported_action("pane.inspect"),
        PaneCommand::Split(_) => unsupported_action("pane.split"),
        PaneCommand::Focus(_) => unsupported_action("pane.focus"),
        PaneCommand::Navigate(_) => unsupported_action("pane.navigate"),
        PaneCommand::Resize(_) => unsupported_action("pane.resize"),
        PaneCommand::Maximize(_) => unsupported_action("pane.maximize"),
        PaneCommand::Unmaximize(_) => unsupported_action("pane.unmaximize"),
        PaneCommand::Close(_) => unsupported_action("pane.close"),
        PaneCommand::Rename(_) => unsupported_action("pane.rename"),
        PaneCommand::ResetName(_) => unsupported_action("pane.reset-name"),
    }
}

pub(super) fn run_session_command(
    command: SessionCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        SessionCommand::List(args) => {
            run_action(args, ActionKind::SessionList, json!({}), output_format)
        }
        SessionCommand::Inspect(_) => unsupported_action("session.inspect"),
        SessionCommand::Activate(_) => unsupported_action("session.activate"),
        SessionCommand::Previous(_) => unsupported_action("session.previous"),
        SessionCommand::Next(_) => unsupported_action("session.next"),
        SessionCommand::ReopenClosed(_) => unsupported_action("session.reopen-closed"),
    }
}

pub(super) fn run_block_command(command: BlockCommand) -> Result<(), ControlError> {
    match command {
        BlockCommand::List(_) => unsupported_action("block.list"),
        BlockCommand::Inspect(_) => unsupported_action("block.inspect"),
        BlockCommand::Output(_) => unsupported_action("block.output"),
    }
}

pub(super) fn run_input_command(
    command: InputCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        InputCommand::Get(_) => unsupported_action("input.get"),
        InputCommand::Insert(args) => run_action(
            args.target,
            ActionKind::InputInsert,
            json!({ "text": args.text }),
            output_format,
        ),
        InputCommand::Replace(args) => run_action(
            args.target,
            ActionKind::InputReplace,
            json!({ "text": args.text }),
            output_format,
        ),
        InputCommand::Clear(args) => {
            run_action(args, ActionKind::InputClear, json!({}), output_format)
        }
        InputCommand::Mode(_) => unsupported_action("input.mode.set"),
    }
}

pub(super) fn run_history_command(command: HistoryCommand) -> Result<(), ControlError> {
    match command {
        HistoryCommand::List(_) => unsupported_action("history.list"),
    }
}

pub(super) fn run_theme_command(
    command: ThemeCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        ThemeCommand::List(args) => {
            run_action(args, ActionKind::ThemeList, json!({}), output_format)
        }
        ThemeCommand::Get(_) => unsupported_action("theme.get"),
        ThemeCommand::Set(args) => run_action(
            args.target,
            ActionKind::ThemeSet,
            json!({ "theme_name": args.theme_name }),
            output_format,
        ),
        ThemeCommand::System(_) => unsupported_action("theme.system.set"),
        ThemeCommand::Light(_) => unsupported_action("theme.light.set"),
        ThemeCommand::Dark(_) => unsupported_action("theme.dark.set"),
    }
}

pub(super) fn run_appearance_command(
    command: AppearanceCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        AppearanceCommand::Get(args) => {
            run_action(args, ActionKind::AppearanceGet, json!({}), output_format)
        }
        AppearanceCommand::FontSize(_) => unsupported_action("appearance.font-size"),
        AppearanceCommand::Zoom(_) => unsupported_action("appearance.zoom"),
    }
}

pub(super) fn run_setting_command(
    command: SettingCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        SettingCommand::List(args) => run_action(
            args.target,
            ActionKind::SettingList,
            json!({ "namespace": args.namespace }),
            output_format,
        ),
        SettingCommand::Get(args) => run_action(
            args.target,
            ActionKind::SettingGet,
            json!({ "key": args.key }),
            output_format,
        ),
        SettingCommand::Set(args) => run_action(
            args.target,
            ActionKind::SettingSet,
            json!({ "key": args.key, "value": args.value }),
            output_format,
        ),
        SettingCommand::Toggle(args) => run_action(
            args.target,
            ActionKind::SettingToggle,
            json!({ "key": args.key }),
            output_format,
        ),
    }
}

pub(super) fn run_keybinding_command(command: KeybindingCommand) -> Result<(), ControlError> {
    match command {
        KeybindingCommand::List(_) => unsupported_action("keybinding.list"),
        KeybindingCommand::Get(_) => unsupported_action("keybinding.get"),
    }
}

pub(super) fn run_action_command(
    command: ActionCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        ActionCommand::List(_) => write_action_metadata(output_format),
        ActionCommand::Inspect(args) => write_named_action_metadata(&args.action, output_format),
    }
}

pub(super) fn run_file_command(command: FileCommand) -> Result<(), ControlError> {
    match command {
        FileCommand::List(_) => unsupported_action("file.list"),
        FileCommand::Open(_) => unsupported_action("file.open"),
    }
}

pub(super) fn run_project_command(command: ProjectCommand) -> Result<(), ControlError> {
    match command {
        ProjectCommand::Active(_) => unsupported_action("project.active"),
        ProjectCommand::List(_) => unsupported_action("project.list"),
        ProjectCommand::Open(_) => unsupported_action("project.open"),
    }
}

pub(super) fn run_drive_command(command: DriveCommand) -> Result<(), ControlError> {
    match command {
        DriveCommand::List(_) => unsupported_action("drive.list"),
        DriveCommand::Inspect(_) => unsupported_action("drive.inspect"),
        DriveCommand::Open(_) => unsupported_action("drive.open"),
        DriveCommand::Notebook(_) => unsupported_action("drive.notebook.open"),
        DriveCommand::EnvVarCollection(_) => unsupported_action("drive.env-var-collection.open"),
        DriveCommand::Object(_) => unsupported_action("drive.object"),
        DriveCommand::Workflow(_) => unsupported_action("drive.workflow"),
    }
}

pub(super) fn run_surface_command(command: SurfaceCommand) -> Result<(), ControlError> {
    match command {
        SurfaceCommand::Settings(_) => unsupported_action("surface.settings.open"),
        SurfaceCommand::CommandPalette(_) => unsupported_action("surface.command-palette.open"),
        SurfaceCommand::CommandSearch(_) => unsupported_action("surface.command-search.open"),
        SurfaceCommand::WarpDrive(_) => unsupported_action("surface.warp-drive"),
        SurfaceCommand::ResourceCenter(_) => unsupported_action("surface.resource-center.toggle"),
        SurfaceCommand::AiAssistant(_) => unsupported_action("surface.ai-assistant.toggle"),
        SurfaceCommand::CodeReview(_) => unsupported_action("surface.code-review.toggle"),
        SurfaceCommand::LeftPanel(_) => unsupported_action("surface.left-panel.toggle"),
        SurfaceCommand::RightPanel(_) => unsupported_action("surface.right-panel.toggle"),
        SurfaceCommand::VerticalTabs(_) => unsupported_action("surface.vertical-tabs.toggle"),
    }
}

pub(super) fn run_auth_command(command: AuthCommand) -> Result<(), ControlError> {
    match command {
        AuthCommand::Status(_) => unsupported_action("auth.status"),
        AuthCommand::Login(_) => unsupported_action("auth.login"),
        AuthCommand::ApiKey(_) => unsupported_action("auth.api-key"),
    }
}

fn run_tab_create(args: TabCreateArgs, output_format: OutputFormat) -> Result<(), ControlError> {
    match (args.tab_type, args.shell.as_ref()) {
        (TabType::Terminal, None) => run_action(
            args.target,
            ActionKind::TabCreate,
            serde_json::Value::Object(Default::default()),
            output_format,
        ),
        (TabType::Terminal, Some(_))
        | (TabType::Agent, None | Some(_))
        | (TabType::CloudAgent, None | Some(_))
        | (TabType::Default, None | Some(_)) => unsupported_action("tab.create.with-options"),
    }
}

fn run_action(
    args: TargetArgs,
    action: ActionKind,
    params: serde_json::Value,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    if !action.is_implemented() {
        return unsupported_action(action.as_str());
    }
    let records = local_control::discovery::list_instances();
    let selector = instance_selector(args.clone());
    let instance = select_instance(&records, &selector)?;
    let mut request = RequestEnvelope::new(Action {
        kind: action,
        params,
    });
    request.target = target_selector(args)?;
    let response = local_control::client::send_request(&instance, &request)?;
    let local_control::protocol::ControlResponse::Ok { data } = response.response else {
        return Err(ControlError::new(
            ErrorCode::Internal,
            "local-control request failed without an error payload",
        ));
    };
    write_value(&data, output_format)
}

fn write_action_metadata(output_format: OutputFormat) -> Result<(), ControlError> {
    let metadata = ActionKind::ALL
        .iter()
        .copied()
        .map(ActionKind::metadata)
        .collect::<Vec<_>>();
    write_values(metadata, output_format)
}

fn write_named_action_metadata(
    action: &str,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    let metadata = ActionKind::ALL
        .iter()
        .copied()
        .map(ActionKind::metadata)
        .find(|metadata| metadata.name == action)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::UnsupportedAction,
                format!("{action} is not in the local-control action catalog"),
            )
        })?;
    write_value(&metadata, output_format)
}

fn write_values<T: Serialize>(
    values: Vec<T>,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match output_format {
        OutputFormat::Json => write_json(&values),
        OutputFormat::Ndjson => {
            for value in values {
                write_json_line(&value)?;
            }
            Ok(())
        }
        OutputFormat::Pretty | OutputFormat::Text => write_json(&values),
    }
}

fn write_value(value: &impl Serialize, output_format: OutputFormat) -> Result<(), ControlError> {
    match output_format {
        OutputFormat::Json | OutputFormat::Pretty => write_json(value),
        OutputFormat::Ndjson | OutputFormat::Text => write_json_line(value),
    }
}

fn unsupported_action(action: &str) -> Result<(), ControlError> {
    let summary = StubSummary {
        ok: false,
        action,
        implemented: false,
    };
    let details = serde_json::to_string(&summary).map_err(|err| {
        ControlError::with_details(
            ErrorCode::Internal,
            "failed to render unsupported local-control action details",
            err.to_string(),
        )
    })?;
    Err(ControlError::with_details(
        ErrorCode::UnsupportedAction,
        format!(
            "{action} is part of the warpctrl command surface but is not implemented by this shard"
        ),
        details,
    ))
}
