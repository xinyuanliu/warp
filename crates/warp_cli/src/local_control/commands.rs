//! Implementations for user-facing `warpctrl` command groups.
use local_control::protocol::{
    Action, ActionGetParams, ActionKind, ActionMetadata, AppFocusParams, AppSurfaceParams,
    AppearanceFontSizeParams, AppearanceSetParams, AppearanceZoomParams, ControlError,
    DriveCreateParams, DriveDeleteParams, DriveGetParams, DriveInsertParams, DriveListParams,
    DriveRunParams, DriveUpdateParams, EmptyParams, ErrorCode, FileDeleteParams, FileListParams,
    FileOpenParams, FileWriteParams, HorizontalDirection, InputClearParams, InputInsertParams,
    InputMode, InputModeSetParams, InputReplaceParams, PaneCloseParams, PaneDirection,
    PaneFocusParams, PaneMaximizeParams, PaneNavigateParams, PaneResizeParams, PaneSplitParams,
    ProjectActiveParams, ProjectListParams, RequestEnvelope, SettingGetParams, SettingListParams,
    SettingSetParams, SettingToggleParams, SizeAdjustment, TabActivateParams, TabActivationTarget,
    TabCloseParams, TabCloseScope, TabMoveParams, TabRenameParams, ThemeSetParams,
    WindowCloseParams, WindowCreateParams, WindowFocusParams,
};
use local_control::selection::select_instance;
use serde::Serialize;
use serde_json::json;

use crate::agent::OutputFormat;
use crate::local_control::auth_commands::{ApiKeySubcommand, AuthCommand};
use crate::local_control::output::{write_json, write_json_line};
use crate::local_control::selectors::{instance_selector, target_selector};
use crate::local_control::{
    ActionCommand, AppCommand, AppSurfaceArgs, AppearanceCommand, BlockCommand, DriveCommand,
    FileCommand, HistoryCommand, InputCommand, InstanceCommand, PaneCommand, ProjectCommand,
    SessionCommand, SettingCommand, TabCommand, TargetArgs, ThemeCommand, WindowCommand,
    parse_json_value_or_string,
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
            match output_format {
                OutputFormat::Json => write_json(&summaries),
                OutputFormat::Ndjson => {
                    for summary in summaries {
                        write_json_line(&summary)?;
                    }
                    Ok(())
                }
                OutputFormat::Pretty | OutputFormat::Text => {
                    for summary in summaries {
                        let endpoint = summary
                            .endpoint
                            .as_ref()
                            .map(|endpoint| format!("{}:{}", endpoint.host, endpoint.port))
                            .unwrap_or_else(|| "outside_warp_disabled".to_owned());
                        println!(
                            "{}\tpid={}\t{}\t{}",
                            summary.instance_id, summary.pid, summary.channel, endpoint
                        );
                    }
                    Ok(())
                }
            }
        }
    }
}

fn run_app_surface_command(
    args: AppSurfaceArgs,
    action: ActionKind,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    run_action_with_params(
        args.target,
        action,
        AppSurfaceParams {
            query: args.query,
            page: args.page,
        },
        output_format,
    )
}

fn run_tab_activate_relative(
    args: TargetArgs,
    relative: TabActivationTarget,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    run_action_with_params(
        args,
        ActionKind::TabActivate,
        TabActivateParams {
            relative: Some(relative),
        },
        output_format,
    )
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
        AppCommand::Active(args) => run_action_with_params(
            args,
            ActionKind::AppActive,
            local_control::AppActiveParams::default(),
            output_format,
        ),
        AppCommand::Inspect(args) => run_action_with_params(
            args,
            ActionKind::AppInspect,
            local_control::AppInspectParams::default(),
            output_format,
        ),
        AppCommand::Focus(args) => run_action_with_params(
            args,
            ActionKind::AppFocus,
            AppFocusParams::default(),
            output_format,
        ),
        AppCommand::SettingsOpen(args) => {
            run_app_surface_command(args, ActionKind::AppSettingsOpen, output_format)
        }
        AppCommand::CommandPaletteOpen(args) => {
            run_app_surface_command(args, ActionKind::AppCommandPaletteOpen, output_format)
        }
        AppCommand::CommandSearchOpen(args) => {
            run_app_surface_command(args, ActionKind::AppCommandSearchOpen, output_format)
        }
        AppCommand::WarpDriveOpen(args) => {
            run_app_surface_command(args, ActionKind::AppWarpDriveOpen, output_format)
        }
        AppCommand::WarpDriveToggle(args) => {
            run_app_surface_command(args, ActionKind::AppWarpDriveToggle, output_format)
        }
        AppCommand::ResourceCenterToggle(args) => {
            run_app_surface_command(args, ActionKind::AppResourceCenterToggle, output_format)
        }
        AppCommand::AiAssistantToggle(args) => {
            run_app_surface_command(args, ActionKind::AppAiAssistantToggle, output_format)
        }
        AppCommand::CodeReviewToggle(args) => {
            run_app_surface_command(args, ActionKind::AppCodeReviewToggle, output_format)
        }
        AppCommand::VerticalTabsToggle(args) => {
            run_app_surface_command(args, ActionKind::AppVerticalTabsToggle, output_format)
        }
    }
}

pub(super) fn run_action_command(
    command: ActionCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        ActionCommand::List(args) => run_action_with_params(
            args,
            ActionKind::ActionList,
            local_control::ActionListParams::default(),
            output_format,
        ),
        ActionCommand::Get(args) => run_action_with_params(
            args.target,
            ActionKind::ActionGet,
            ActionGetParams {
                action: args.action,
            },
            output_format,
        ),
    }
}

pub(super) fn run_window_command(
    command: WindowCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        WindowCommand::List(args) => {
            run_action_with_params(args, ActionKind::WindowList, EmptyParams {}, output_format)
        }
        WindowCommand::Create(args) => run_action_with_params(
            args.target,
            ActionKind::WindowCreate,
            WindowCreateParams {
                profile: args.profile,
            },
            output_format,
        ),
        WindowCommand::Focus(args) => run_action_with_params(
            args,
            ActionKind::WindowFocus,
            WindowFocusParams::default(),
            output_format,
        ),
        WindowCommand::Close(args) => run_action_with_params(
            args.target,
            ActionKind::WindowClose,
            WindowCloseParams { force: args.force },
            output_format,
        ),
    }
}
pub(super) fn run_tab_command(
    command: TabCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        TabCommand::List(args) => {
            run_action_with_params(args, ActionKind::TabList, EmptyParams {}, output_format)
        }
        TabCommand::Create(args) => {
            run_action(args, ActionKind::TabCreate, json!({}), output_format)
        }
        TabCommand::Activate(args) => run_action_with_params(
            args,
            ActionKind::TabActivate,
            TabActivateParams { relative: None },
            output_format,
        ),
        TabCommand::Previous(args) => {
            run_tab_activate_relative(args, TabActivationTarget::Previous, output_format)
        }
        TabCommand::Next(args) => {
            run_tab_activate_relative(args, TabActivationTarget::Next, output_format)
        }
        TabCommand::Last(args) => {
            run_tab_activate_relative(args, TabActivationTarget::Last, output_format)
        }
        TabCommand::Move(args) => run_action_with_params(
            args.target,
            ActionKind::TabMove,
            TabMoveParams {
                direction: HorizontalDirection::from(args.direction),
            },
            output_format,
        ),
        TabCommand::Rename(args) => run_action_with_params(
            args.target,
            ActionKind::TabRename,
            TabRenameParams {
                title: if args.reset { None } else { args.title },
            },
            output_format,
        ),
        TabCommand::Close(args) => run_action_with_params(
            args.target,
            ActionKind::TabClose,
            TabCloseParams {
                scope: TabCloseScope::from(args.scope),
                force: args.force,
            },
            output_format,
        ),
    }
}

pub(super) fn run_pane_command(
    command: PaneCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        PaneCommand::List(args) => {
            run_action_with_params(args, ActionKind::PaneList, EmptyParams {}, output_format)
        }
        PaneCommand::Split(args) => run_action_with_params(
            args.target,
            ActionKind::PaneSplit,
            PaneSplitParams {
                direction: PaneDirection::from(args.direction),
                profile: args.profile,
            },
            output_format,
        ),
        PaneCommand::Focus(args) => run_action_with_params(
            args,
            ActionKind::PaneFocus,
            PaneFocusParams::default(),
            output_format,
        ),
        PaneCommand::Navigate(args) => run_action_with_params(
            args.target,
            ActionKind::PaneNavigate,
            PaneNavigateParams {
                direction: PaneDirection::from(args.direction),
            },
            output_format,
        ),
        PaneCommand::Close(args) => run_action_with_params(
            args.target,
            ActionKind::PaneClose,
            PaneCloseParams { force: args.force },
            output_format,
        ),
        PaneCommand::Maximize(args) => run_action_with_params(
            args.target,
            ActionKind::PaneMaximize,
            PaneMaximizeParams {
                enabled: args.enabled,
            },
            output_format,
        ),
        PaneCommand::Resize(args) => run_action_with_params(
            args.target,
            ActionKind::PaneResize,
            PaneResizeParams {
                direction: PaneDirection::from(args.direction),
                amount: args.amount,
            },
            output_format,
        ),
        PaneCommand::PreviousSession(args) => run_action_with_params(
            args,
            ActionKind::PaneSessionPrevious,
            EmptyParams {},
            output_format,
        ),
        PaneCommand::NextSession(args) => run_action_with_params(
            args,
            ActionKind::PaneSessionNext,
            EmptyParams {},
            output_format,
        ),
    }
}

pub(super) fn run_session_command(
    command: SessionCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        SessionCommand::List(args) => {
            run_action_with_params(args, ActionKind::SessionList, EmptyParams {}, output_format)
        }
        SessionCommand::Activate(args) => run_action_with_params(
            args,
            ActionKind::SessionActivate,
            EmptyParams {},
            output_format,
        ),
        SessionCommand::Previous(args) => run_action_with_params(
            args,
            ActionKind::SessionPrevious,
            EmptyParams {},
            output_format,
        ),
        SessionCommand::Next(args) => {
            run_action_with_params(args, ActionKind::SessionNext, EmptyParams {}, output_format)
        }
        SessionCommand::ReopenClosed(args) => run_action_with_params(
            args,
            ActionKind::SessionReopen,
            EmptyParams {},
            output_format,
        ),
    }
}

pub(super) fn run_block_command(
    command: BlockCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        BlockCommand::List(args) => run_action_with_params(
            args.target,
            ActionKind::BlockList,
            local_control::BlockListParams { limit: args.limit },
            output_format,
        ),
        BlockCommand::Get(args) => run_action_with_params(
            args.target,
            ActionKind::BlockGet,
            local_control::BlockGetParams {
                block_id: args.block_id,
            },
            output_format,
        ),
    }
}

pub(super) fn run_input_command(
    command: InputCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        InputCommand::Get(args) => run_action_with_params(
            args,
            ActionKind::InputGet,
            local_control::InputGetParams::default(),
            output_format,
        ),
        InputCommand::Insert(args) => run_action_with_params(
            args.target,
            ActionKind::InputInsert,
            InputInsertParams {
                text: args.text,
                replace: args.replace,
            },
            output_format,
        ),
        InputCommand::Replace(args) => run_action_with_params(
            args.target,
            ActionKind::InputReplace,
            InputReplaceParams { text: args.text },
            output_format,
        ),
        InputCommand::Clear(args) => run_action_with_params(
            args,
            ActionKind::InputClear,
            InputClearParams::default(),
            output_format,
        ),
        InputCommand::Mode(args) => run_action_with_params(
            args.target,
            ActionKind::InputModeSet,
            InputModeSetParams {
                mode: InputMode::from(args.mode),
            },
            output_format,
        ),
    }
}

pub(super) fn run_theme_command(
    command: ThemeCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        ThemeCommand::List(args) => {
            run_action_with_params(args, ActionKind::ThemeList, EmptyParams {}, output_format)
        }
        ThemeCommand::Set(args) => run_action_with_params(
            args.target,
            ActionKind::ThemeSet,
            ThemeSetParams { name: args.name },
            output_format,
        ),
    }
}

pub(super) fn run_appearance_command(
    command: AppearanceCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        AppearanceCommand::Get(args) => run_action_with_params(
            args,
            ActionKind::AppearanceGet,
            EmptyParams {},
            output_format,
        ),
        AppearanceCommand::Set(args) => run_action_with_params(
            args.target,
            ActionKind::AppearanceSet,
            AppearanceSetParams {
                theme: args.theme,
                follow_system_theme: args.follow_system_theme,
                light_theme: args.light_theme,
                dark_theme: args.dark_theme,
            },
            output_format,
        ),
        AppearanceCommand::FontSize(args) => run_action_with_params(
            args.target,
            ActionKind::AppearanceFontSize,
            AppearanceFontSizeParams {
                adjustment: SizeAdjustment::from(args.adjustment),
                value: args.value,
            },
            output_format,
        ),
        AppearanceCommand::Zoom(args) => run_action_with_params(
            args.target,
            ActionKind::AppearanceZoom,
            AppearanceZoomParams {
                adjustment: SizeAdjustment::from(args.adjustment),
                value: args.value,
            },
            output_format,
        ),
    }
}

pub(super) fn run_history_command(
    command: HistoryCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        HistoryCommand::List(args) => run_action_with_params(
            args.target,
            ActionKind::HistoryList,
            local_control::HistoryListParams { limit: args.limit },
            output_format,
        ),
    }
}
pub(super) fn run_setting_command(
    command: SettingCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        SettingCommand::List(args) => run_action_with_params(
            args,
            ActionKind::SettingList,
            SettingListParams::default(),
            output_format,
        ),
        SettingCommand::Get(args) => run_action_with_params(
            args.target,
            ActionKind::SettingGet,
            SettingGetParams { key: args.key },
            output_format,
        ),
        SettingCommand::Set(args) => run_action_with_params(
            args.target,
            ActionKind::SettingSet,
            SettingSetParams {
                key: args.key,
                value: parse_json_value_or_string(args.value),
            },
            output_format,
        ),
        SettingCommand::Toggle(args) => run_action_with_params(
            args.target,
            ActionKind::SettingToggle,
            SettingToggleParams { key: args.key },
            output_format,
        ),
    }
}

pub(super) fn run_file_command(
    command: FileCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        FileCommand::List(args) => run_action_with_params(
            args,
            ActionKind::FileList,
            FileListParams::default(),
            output_format,
        ),
        FileCommand::Open(args) => run_action_with_params(
            args.target,
            ActionKind::FileOpen,
            FileOpenParams {
                path: args.path,
                line: args.line,
                new_window: args.new_window,
            },
            output_format,
        ),
        FileCommand::Write(args) => run_action_with_params(
            args.target,
            ActionKind::FileWrite,
            FileWriteParams {
                path: args.path,
                contents: args.contents,
                create: args.create,
            },
            output_format,
        ),
        FileCommand::Delete(args) => run_action_with_params(
            args.target,
            ActionKind::FileDelete,
            FileDeleteParams {
                path: args.path,
                recursive: args.recursive,
            },
            output_format,
        ),
    }
}

pub(super) fn run_project_command(
    command: ProjectCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        ProjectCommand::Active(args) => run_action_with_params(
            args,
            ActionKind::ProjectActive,
            ProjectActiveParams::default(),
            output_format,
        ),
        ProjectCommand::List(args) => run_action_with_params(
            args,
            ActionKind::ProjectList,
            ProjectListParams::default(),
            output_format,
        ),
    }
}

pub(super) fn run_drive_command(
    command: DriveCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        DriveCommand::List(args) => run_action_with_params(
            args.target,
            ActionKind::DriveList,
            DriveListParams {
                object_type: args.object_type.map(|t| t.into()),
            },
            output_format,
        ),
        DriveCommand::Get(args) => run_action_with_params(
            args.target,
            ActionKind::DriveGet,
            DriveGetParams {
                object_type: args.object_type.into(),
                id: args.id,
            },
            output_format,
        ),
        DriveCommand::Create(args) => run_action_with_params(
            args.target,
            ActionKind::DriveCreate,
            DriveCreateParams {
                object_type: args.object_type.into(),
                name: args.name,
                content: parse_json_value_or_string(args.content),
            },
            output_format,
        ),
        DriveCommand::Update(args) => run_action_with_params(
            args.target,
            ActionKind::DriveUpdate,
            DriveUpdateParams {
                object_type: args.object_type.into(),
                id: args.id,
                content: parse_json_value_or_string(args.content),
            },
            output_format,
        ),
        DriveCommand::Delete(args) => run_action_with_params(
            args.target,
            ActionKind::DriveDelete,
            DriveDeleteParams {
                object_type: args.object_type.into(),
                id: args.id,
            },
            output_format,
        ),
        DriveCommand::Run(args) => run_action_with_params(
            args.target,
            ActionKind::DriveRun,
            DriveRunParams {
                object_type: args.object_type.into(),
                id: args.id,
            },
            output_format,
        ),
        DriveCommand::Insert(args) => run_action_with_params(
            args.target,
            ActionKind::DriveInsert,
            DriveInsertParams {
                object_type: args.object_type.into(),
                id: args.id,
            },
            output_format,
        ),
    }
}

pub(super) fn run_auth_command(
    command: AuthCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        AuthCommand::Status(args) => {
            let records = local_control::discovery::list_instances();
            let selector = instance_selector(&args);
            let instance = select_instance(&records, &selector)?;
            let request = RequestEnvelope::new(Action::new(ActionKind::AppVersion));
            let response = local_control::client::send_request(&instance, &request)?;
            let local_control::protocol::ControlResponse::Ok { data } = response.response else {
                return Err(ControlError::new(
                    ErrorCode::Internal,
                    "auth status request failed",
                ));
            };
            match output_format {
                OutputFormat::Json => write_json(&data),
                OutputFormat::Ndjson => write_json_line(&data),
                OutputFormat::Pretty | OutputFormat::Text => write_json(&data),
            }
        }
        AuthCommand::Login(args) => {
            let records = local_control::discovery::list_instances();
            let selector = instance_selector(&args);
            select_instance(&records, &selector)?;
            Err(ControlError::new(
                ErrorCode::UnsupportedAction,
                "warpctrl auth login is not yet implemented; open Settings > Account in Warp to log in",
            ))
        }
        AuthCommand::ApiKey(subcommand) => run_api_key_command(subcommand, output_format),
    }
}

fn run_api_key_command(
    command: ApiKeySubcommand,
    _output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        ApiKeySubcommand::Set(_args) => Err(ControlError::new(
            ErrorCode::UnsupportedAction,
            "warpctrl auth api-key set is not yet implemented; external API-key exchange requires authenticated scripting support",
        )),
        ApiKeySubcommand::Status(_args) => Err(ControlError::new(
            ErrorCode::UnsupportedAction,
            "warpctrl auth api-key status is not yet implemented",
        )),
        ApiKeySubcommand::Revoke(_args) => Err(ControlError::new(
            ErrorCode::UnsupportedAction,
            "warpctrl auth api-key revoke is not yet implemented",
        )),
    }
}

fn run_action(
    args: TargetArgs,
    action: ActionKind,
    params: serde_json::Value,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    let records = local_control::discovery::list_instances();
    let selector = instance_selector(&args);
    let instance = select_instance(&records, &selector)?;
    let mut request = RequestEnvelope::new(Action {
        kind: action,
        params,
    });
    request.target = target_selector(&args);
    let response = local_control::client::send_request(&instance, &request)?;
    let local_control::protocol::ControlResponse::Ok { data } = response.response else {
        return Err(ControlError::new(
            ErrorCode::Internal,
            "local-control request failed without an error payload",
        ));
    };
    match output_format {
        OutputFormat::Json => write_json(&data),
        OutputFormat::Ndjson => write_json_line(&data),
        OutputFormat::Pretty | OutputFormat::Text => write_json(&data),
    }
}

fn run_action_with_params<T: Serialize>(
    args: TargetArgs,
    action: ActionKind,
    params: T,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    let records = local_control::discovery::list_instances();
    let selector = instance_selector(&args);
    let instance = select_instance(&records, &selector)?;
    let mut request = RequestEnvelope::new(Action::with_params(action, params)?);
    request.target = target_selector(&args);
    let response = local_control::client::send_request(&instance, &request)?;
    let local_control::protocol::ControlResponse::Ok { data } = response.response else {
        return Err(ControlError::new(
            ErrorCode::Internal,
            "local-control request failed without an error payload",
        ));
    };
    match output_format {
        OutputFormat::Json => write_json(&data),
        OutputFormat::Ndjson => write_json_line(&data),
        OutputFormat::Pretty | OutputFormat::Text => write_json(&data),
    }
}
