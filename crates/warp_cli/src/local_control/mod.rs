//! Command-line interface for controlling a running local Warp app.
mod auth_commands;
mod commands;
mod completions;
mod output;
mod selectors;

use std::process::ExitCode;

use crate::agent::OutputFormat;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use clap_complete::aot::Shell;

pub use auth_commands::{ApiKeySetArgs, ApiKeySourceArgs, ApiKeySubcommand, AuthCommand};
use commands::{
    run_action_command, run_app_command, run_appearance_command, run_auth_command,
    run_block_command, run_drive_command, run_file_command, run_history_command, run_input_command,
    run_instance_command, run_pane_command, run_project_command, run_session_command,
    run_setting_command, run_tab_command, run_theme_command, run_window_command,
};
use completions::generate_completions_to_stdout;
use output::write_control_error;

/// Parsed top-level arguments for `warpctrl`.
#[derive(Debug, Parser)]
#[command(
    name = "warpctrl",
    display_name = "warpctrl",
    about = "Control a running local Warp app instance"
)]
pub struct ControlArgs {
    /// Set the output format.
    #[arg(
        long = "output-format",
        global = true,
        value_enum,
        default_value_t = OutputFormat::Pretty,
        env = "WARP_OUTPUT_FORMAT"
    )]
    pub output_format: OutputFormat,

    #[command(subcommand)]
    pub command: ControlCommand,
}

impl ControlArgs {
    pub fn from_env() -> Self {
        let matches = Self::clap_command().get_matches();
        Self::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
    }

    pub fn clap_command() -> clap::Command {
        let bin_name = crate::binary_name().unwrap_or_else(|| "warpctrl".to_owned());
        <Self as CommandFactory>::command()
            .version(crate::version_string())
            .bin_name(bin_name.clone())
            .after_help(color_print::cformat!(
                r#"<bold><underline>Examples:</underline></bold>

  <dim>$</dim> <bold>{bin_name} instance list</bold>

  <dim>$</dim> <bold>{bin_name} tab create</bold>

<bold><underline>Learn more:</underline></bold>
* Use <bold>{bin_name} help</bold> to learn more about each command
"#
            ))
    }
}

/// Top-level `warpctrl` command groups.
#[derive(Debug, Clone, Subcommand)]
pub enum ControlCommand {
    /// Inspect local Warp app instances.
    #[command(subcommand)]
    Instance(InstanceCommand),
    /// Inspect a selected local Warp app.
    #[command(subcommand)]
    App(AppCommand),
    /// Inspect the local-control action catalog.
    #[command(subcommand)]
    Action(ActionCommand),

    /// Inspect local Warp windows.
    #[command(subcommand)]
    Window(WindowCommand),

    /// Control local Warp tabs.
    #[command(subcommand)]
    Tab(TabCommand),
    /// Inspect local Warp panes.
    #[command(subcommand)]
    Pane(PaneCommand),

    /// Inspect local Warp sessions.
    #[command(subcommand)]
    Session(SessionCommand),

    /// Inspect terminal blocks.
    #[command(subcommand)]
    Block(BlockCommand),

    /// Inspect terminal input state.
    #[command(subcommand)]
    Input(InputCommand),

    /// Inspect terminal command history.
    #[command(subcommand)]
    History(HistoryCommand),
    /// Inspect Warp themes.
    #[command(subcommand)]
    Theme(ThemeCommand),

    /// Inspect appearance state.
    #[command(subcommand)]
    Appearance(AppearanceCommand),

    /// Inspect allowlisted settings.
    #[command(subcommand)]
    Setting(SettingCommand),

    /// Inspect files currently surfaced in Warp.
    #[command(subcommand)]
    File(FileCommand),

    /// Inspect projects currently known to Warp.
    #[command(subcommand)]
    Project(ProjectCommand),

    /// Inspect Warp Drive objects.
    #[command(subcommand)]
    Drive(DriveCommand),

    /// Manage authenticated scripting identity and API keys.
    #[command(subcommand)]
    Auth(AuthCommand),

    /// Generate shell completions for your shell to stdout.
    ///
    /// For bash, add the following to ~/.bashrc:
    ///     source <(path/to/warpctrl completions bash)
    ///
    /// For zsh, add the following to ~/.zshrc:
    ///     source <(path/to/warpctrl completions zsh)
    ///
    /// For fish, add the following to ~/.config/fish/config.fish:
    ///     path/to/warpctrl completions fish | source
    ///
    /// For Powershell, add the following to $PROFILE:
    ///     path\to\warpctrl completions powershell | Out-String | Invoke-Expression
    ///
    /// If no shell is provided, this defaults to the shell that Warp was run from.
    #[command(verbatim_doc_comment)]
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Option<Shell>,
    },
}

/// Commands that inspect locally discoverable Warp instances.
#[derive(Debug, Clone, Subcommand)]
pub enum InstanceCommand {
    /// List locally discoverable Warp instances.
    List,
}

/// Commands that inspect the selected Warp app instance.
#[derive(Debug, Clone, Subcommand)]
pub enum AppCommand {
    /// Check that the selected local Warp app responds.
    Ping(TargetArgs),

    /// Print protocol and app version metadata for the selected local Warp app.
    Version(TargetArgs),

    /// Print the active window/tab/pane/session chain.
    Active(TargetArgs),

    /// Print app and protocol metadata.
    Inspect(TargetArgs),

    /// Focus the selected Warp app instance.
    Focus(TargetArgs),

    /// Open the Settings surface.
    SettingsOpen(AppSurfaceArgs),

    /// Open the Command Palette.
    CommandPaletteOpen(AppSurfaceArgs),

    /// Open command search.
    CommandSearchOpen(AppSurfaceArgs),

    /// Open Warp Drive.
    WarpDriveOpen(AppSurfaceArgs),

    /// Toggle Warp Drive.
    WarpDriveToggle(AppSurfaceArgs),

    /// Toggle the resource center.
    ResourceCenterToggle(AppSurfaceArgs),

    /// Toggle the AI assistant surface.
    AiAssistantToggle(AppSurfaceArgs),

    /// Toggle the code review surface.
    CodeReviewToggle(AppSurfaceArgs),

    /// Toggle the vertical tabs panel.
    VerticalTabsToggle(AppSurfaceArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum ActionCommand {
    /// List allowlisted local-control actions.
    List(TargetArgs),

    /// Inspect one allowlisted local-control action.
    Get(ActionGetArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum WindowCommand {
    /// List windows in the selected local Warp app.
    List(TargetArgs),

    /// Create a new Warp window.
    Create(WindowCreateArgs),

    /// Focus a Warp window.
    Focus(TargetArgs),

    /// Close a Warp window.
    Close(WindowCloseArgs),
}

/// Commands that control tabs in the selected Warp app instance.
#[derive(Debug, Clone, Subcommand)]
pub enum TabCommand {
    /// List tabs in the selected local Warp app.
    List(TargetArgs),
    /// Create a new terminal tab in the active window.
    Create(TargetArgs),
    /// Activate a target tab.
    Activate(TargetArgs),
    /// Activate the previous tab.
    Previous(TargetArgs),
    /// Activate the next tab.
    Next(TargetArgs),
    /// Activate the last tab.
    Last(TargetArgs),
    /// Move a target tab left or right.
    Move(TabMoveArgs),
    /// Rename or reset a target tab title.
    Rename(TabRenameArgs),
    /// Close a target tab or tab group.
    Close(TabCloseArgs),
}

/// Commands that inspect local Warp panes.
#[derive(Debug, Clone, Subcommand)]
pub enum PaneCommand {
    /// List panes in the selected local Warp app.
    List(TargetArgs),
    /// Split a pane.
    Split(PaneSplitArgs),
    /// Focus a pane.
    Focus(TargetArgs),
    /// Navigate pane focus.
    Navigate(PaneNavigateArgs),
    /// Close a pane.
    Close(PaneCloseArgs),
    /// Toggle or set pane maximization.
    Maximize(PaneMaximizeArgs),
    /// Resize a pane divider.
    Resize(PaneResizeArgs),
    /// Switch to the previous session in a pane.
    PreviousSession(TargetArgs),
    /// Switch to the next session in a pane.
    NextSession(TargetArgs),
}
/// Commands that inspect local Warp sessions.

#[derive(Debug, Clone, Subcommand)]
pub enum SessionCommand {
    /// List sessions in the selected local Warp app.
    List(TargetArgs),
    /// Activate the selected session.
    Activate(TargetArgs),
    /// Switch to the previous session.
    Previous(TargetArgs),
    /// Switch to the next session.
    Next(TargetArgs),
    /// Reopen the most recently closed session.
    ReopenClosed(TargetArgs),
}
/// Commands that inspect terminal blocks.

#[derive(Debug, Clone, Subcommand)]
pub enum BlockCommand {
    /// List terminal blocks.
    List(LimitTargetArgs),

    /// Read one terminal block.
    Get(BlockGetArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum InputCommand {
    /// Read the current input buffer.
    Get(TargetArgs),
    /// Insert text into the active input buffer.
    Insert(InputInsertArgs),
    /// Replace the active input buffer.
    Replace(InputTextArgs),
    /// Clear the active input buffer.
    Clear(TargetArgs),
    /// Set the active input mode.
    Mode(InputModeArgs),
}

/// Commands that inspect Warp themes.
#[derive(Debug, Clone, Subcommand)]
pub enum ThemeCommand {
    /// List available themes.
    List(TargetArgs),
    /// Set the current theme.
    Set(ThemeSetArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum AppearanceCommand {
    /// Read appearance state.
    Get(TargetArgs),
    /// Set theme-following appearance state.
    Set(AppearanceSetArgs),
    /// Adjust font size.
    FontSize(AppearanceAdjustArgs),
    /// Adjust UI zoom.
    Zoom(AppearanceAdjustArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum HistoryCommand {
    /// List command history entries.
    List(LimitTargetArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum SettingCommand {
    /// List allowlisted settings.
    List(TargetArgs),

    /// Read one allowlisted setting.
    Get(SettingGetArgs),

    /// Set one allowlisted setting.
    Set(SettingSetArgsCli),

    /// Toggle one allowlisted boolean setting.
    Toggle(SettingToggleArgsCli),
}

#[derive(Debug, Clone, Subcommand)]
pub enum FileCommand {
    /// List files currently surfaced in Warp.
    List(TargetArgs),
    /// Open a path in Warp.
    Open(FileOpenArgs),
    /// Write a file through the local-control protocol.
    Write(FileWriteArgs),
    /// Delete a file through the local-control protocol.
    Delete(FileDeleteArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProjectCommand {
    /// Print the active project for the selected local Warp app.
    Active(TargetArgs),
    /// List projects currently known to Warp.
    List(TargetArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum DriveCommand {
    /// List Warp Drive objects.
    List(DriveListArgs),
    /// Read a Warp Drive object.
    Get(DriveGetArgs),
    /// Create a Warp Drive object.
    Create(DriveCreateArgs),
    /// Update a Warp Drive object.
    Update(DriveUpdateArgs),
    /// Delete a Warp Drive object.
    Delete(DriveObjectMutationArgs),
    /// Run a Warp Drive workflow or notebook.
    Run(DriveObjectMutationArgs),
    /// Insert a Warp Drive object into the active terminal session.
    Insert(DriveObjectMutationArgs),
}

/// Common flags for selecting which running Warp instance receives a command.
#[derive(Debug, Clone, Args, Default)]
pub struct TargetArgs {
    /// Target a specific local Warp instance id from `warp instance list`.
    #[arg(long = "instance")]
    pub instance: Option<String>,

    /// Target a specific local Warp process id.
    #[arg(long = "pid", conflicts_with = "instance")]
    pub pid: Option<u32>,

    /// Target a session by `active` or opaque session id.
    #[arg(long = "session", conflicts_with = "session_id")]
    pub session: Option<String>,

    /// Target a session by opaque session id.
    #[arg(long = "session-id", conflicts_with = "session")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ActionGetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Action name, such as tab.create or window.list.
    pub action: String,
}

#[derive(Debug, Clone, Args)]
pub struct LimitTargetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Maximum number of items to return.
    #[arg(long = "limit")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Args)]
pub struct BlockGetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Opaque block id returned by block list.
    pub block_id: String,
}

#[derive(Debug, Clone, Args)]
pub struct SettingGetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Allowlisted setting key.
    pub key: String,
}

#[derive(Debug, Clone, Args)]
pub struct SettingSetArgsCli {
    #[command(flatten)]
    pub target: TargetArgs,

    pub key: String,

    pub value: String,
}

#[derive(Debug, Clone, Args)]
pub struct SettingToggleArgsCli {
    #[command(flatten)]
    pub target: TargetArgs,

    pub key: String,
}

#[derive(Debug, Clone, Args)]
pub struct AppSurfaceArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "query")]
    pub query: Option<String>,

    #[arg(long = "page")]
    pub page: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct WindowCreateArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "profile")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct WindowCloseArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TabMoveArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "direction", value_enum)]
    pub direction: HorizontalDirectionArg,
}

#[derive(Debug, Clone, Args)]
pub struct TabRenameArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub title: Option<String>,

    #[arg(long = "reset")]
    pub reset: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TabCloseArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "scope", value_enum, default_value_t = TabCloseScopeArg::Target)]
    pub scope: TabCloseScopeArg,

    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct PaneSplitArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "direction", value_enum)]
    pub direction: PaneDirectionArg,

    #[arg(long = "profile")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct PaneNavigateArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "direction", value_enum)]
    pub direction: PaneDirectionArg,
}

#[derive(Debug, Clone, Args)]
pub struct PaneCloseArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "force")]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct PaneMaximizeArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "enabled")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Args)]
pub struct PaneResizeArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "direction", value_enum)]
    pub direction: PaneDirectionArg,

    #[arg(long = "amount")]
    pub amount: Option<u32>,
}

#[derive(Debug, Clone, Args)]
pub struct InputInsertArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub text: String,

    #[arg(long = "replace")]
    pub replace: bool,
}

#[derive(Debug, Clone, Args)]
pub struct InputTextArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub text: String,
}

#[derive(Debug, Clone, Args)]
pub struct InputModeArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(value_enum)]
    pub mode: InputModeArg,
}


#[derive(Debug, Clone, Args)]
pub struct ThemeSetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub name: String,
}

#[derive(Debug, Clone, Args)]
pub struct AppearanceSetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(long = "theme")]
    pub theme: Option<String>,

    #[arg(long = "follow-system-theme")]
    pub follow_system_theme: Option<bool>,

    #[arg(long = "light-theme")]
    pub light_theme: Option<String>,

    #[arg(long = "dark-theme")]
    pub dark_theme: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct AppearanceAdjustArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    #[arg(value_enum)]
    pub adjustment: SizeAdjustmentArg,

    #[arg(long = "value")]
    pub value: Option<u32>,
}

#[derive(Debug, Clone, Args)]
pub struct FileOpenArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub path: String,

    #[arg(long = "line")]
    pub line: Option<u32>,

    #[arg(long = "new-window")]
    pub new_window: bool,
}

#[derive(Debug, Clone, Args)]
pub struct FileWriteArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub path: String,

    pub contents: String,

    #[arg(long = "create")]
    pub create: bool,
}

#[derive(Debug, Clone, Args)]
pub struct FileDeleteArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    pub path: String,

    #[arg(long = "recursive")]
    pub recursive: bool,
}

#[derive(Debug, Clone, Args)]
pub struct DriveListArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Restrict results to one Drive object type.
    #[arg(long = "type")]
    pub object_type: Option<DriveObjectTypeArg>,
}

#[derive(Debug, Clone, Args)]
pub struct DriveGetArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Warp Drive object type.
    #[arg(long = "type")]
    pub object_type: DriveObjectTypeArg,

    /// Opaque Warp Drive object id.
    pub id: String,
}

#[derive(Debug, Clone, Args)]
pub struct DriveCreateArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Warp Drive object type.
    #[arg(long = "type")]
    pub object_type: DriveObjectTypeArg,

    /// Name for the new Drive object.
    pub name: String,

    /// Object content, parsed as JSON when possible and otherwise treated as a string.
    pub content: String,
}

#[derive(Debug, Clone, Args)]
pub struct DriveUpdateArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Warp Drive object type.
    #[arg(long = "type")]
    pub object_type: DriveObjectTypeArg,

    /// Opaque Warp Drive object id.
    pub id: String,

    /// Object content, parsed as JSON when possible and otherwise treated as a string.
    pub content: String,
}

#[derive(Debug, Clone, Args)]
pub struct DriveObjectMutationArgs {
    #[command(flatten)]
    pub target: TargetArgs,

    /// Warp Drive object type.
    #[arg(long = "type")]
    pub object_type: DriveObjectTypeArg,

    /// Opaque Warp Drive object id.
    pub id: String,
}

use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DriveObjectTypeArg {
    Workflow,
    Notebook,
    Environment,
    Prompt,
}

impl From<DriveObjectTypeArg> for local_control::DriveObjectType {
    fn from(value: DriveObjectTypeArg) -> Self {
        match value {
            DriveObjectTypeArg::Workflow => Self::Workflow,
            DriveObjectTypeArg::Notebook => Self::Notebook,
            DriveObjectTypeArg::Environment => Self::Environment,
            DriveObjectTypeArg::Prompt => Self::Prompt,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HorizontalDirectionArg {
    Left,
    Right,
}

impl From<HorizontalDirectionArg> for local_control::HorizontalDirection {
    fn from(value: HorizontalDirectionArg) -> Self {
        match value {
            HorizontalDirectionArg::Left => Self::Left,
            HorizontalDirectionArg::Right => Self::Right,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TabCloseScopeArg {
    Target,
    Others,
    Right,
}

impl From<TabCloseScopeArg> for local_control::TabCloseScope {
    fn from(value: TabCloseScopeArg) -> Self {
        match value {
            TabCloseScopeArg::Target => Self::Target,
            TabCloseScopeArg::Others => Self::Others,
            TabCloseScopeArg::Right => Self::Right,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PaneDirectionArg {
    Left,
    Right,
    Up,
    Down,
}

impl From<PaneDirectionArg> for local_control::PaneDirection {
    fn from(value: PaneDirectionArg) -> Self {
        match value {
            PaneDirectionArg::Left => Self::Left,
            PaneDirectionArg::Right => Self::Right,
            PaneDirectionArg::Up => Self::Up,
            PaneDirectionArg::Down => Self::Down,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum InputModeArg {
    Terminal,
    Agent,
}

impl From<InputModeArg> for local_control::InputMode {
    fn from(value: InputModeArg) -> Self {
        match value {
            InputModeArg::Terminal => Self::Terminal,
            InputModeArg::Agent => Self::Agent,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SizeAdjustmentArg {
    Increase,
    Decrease,
    Reset,
    Set,
}

impl From<SizeAdjustmentArg> for local_control::SizeAdjustment {
    fn from(value: SizeAdjustmentArg) -> Self {
        match value {
            SizeAdjustmentArg::Increase => Self::Increase,
            SizeAdjustmentArg::Decrease => Self::Decrease,
            SizeAdjustmentArg::Reset => Self::Reset,
            SizeAdjustmentArg::Set => Self::Set,
        }
    }
}

pub(super) fn parse_json_value_or_string(value: String) -> serde_json::Value {
    match serde_json::from_str(&value) {
        Ok(value) => value,
        Err(_) => serde_json::Value::String(value),
    }
}

pub fn run(args: ControlArgs) -> ExitCode {
    let output_format = args.output_format;
    match run_inner(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Err(write_error) = write_control_error(&error, output_format) {
                eprintln!(
                    "error: failed to render local-control error: {}",
                    write_error.message
                );
            }
            ExitCode::FAILURE
        }
    }
}

fn run_inner(args: ControlArgs) -> Result<(), local_control::protocol::ControlError> {
    let output_format = args.output_format;
    match args.command {
        ControlCommand::Instance(command) => run_instance_command(command, output_format),
        ControlCommand::App(command) => run_app_command(command, output_format),
        ControlCommand::Action(command) => run_action_command(command, output_format),
        ControlCommand::Window(command) => run_window_command(command, output_format),
        ControlCommand::Tab(command) => run_tab_command(command, output_format),
        ControlCommand::Pane(command) => run_pane_command(command, output_format),
        ControlCommand::Session(command) => run_session_command(command, output_format),
        ControlCommand::Block(command) => run_block_command(command, output_format),
        ControlCommand::Input(command) => run_input_command(command, output_format),
        ControlCommand::History(command) => run_history_command(command, output_format),
        ControlCommand::Theme(command) => run_theme_command(command, output_format),
        ControlCommand::Appearance(command) => run_appearance_command(command, output_format),
        ControlCommand::Setting(command) => run_setting_command(command, output_format),
        ControlCommand::File(command) => run_file_command(command, output_format),
        ControlCommand::Project(command) => run_project_command(command, output_format),
        ControlCommand::Drive(command) => run_drive_command(command, output_format),
        ControlCommand::Auth(command) => run_auth_command(command, output_format),
        ControlCommand::Completions { shell } => generate_completions_to_stdout(shell),
    }
}

#[cfg(test)]
pub(crate) use completions::generate_completion_string;
#[cfg(test)]
pub(crate) use output::ErrorSummary;

#[cfg(test)]
#[path = "../local_control_tests.rs"]
mod tests;
