//! Guided Warp tour: deterministic stop copy, composite stop commands, and
//! the interactive `tour run` state machine.
mod composite;
mod copy;
mod invoker;
mod runner;
mod state;
#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

use local_control::protocol::ControlError;
pub use state::TourStop;

use crate::agent::OutputFormat;
use crate::local_control::TourCommand;

pub(super) fn run_tour_command(
    command: TourCommand,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    match command {
        TourCommand::Run(args) => runner::run_interactive_tour(&args),
        TourCommand::Init(args) => composite::run_init_command(&args, output_format),
        TourCommand::Stop(args) => composite::run_stop_command(&args, output_format),
        TourCommand::Finish(args) => composite::run_finish_command(&args, output_format),
        TourCommand::Welcome => print_copy(copy::welcome()),
        TourCommand::Themes => print_copy(copy::themes()),
        TourCommand::Keybindings => print_copy(copy::keybindings()),
        TourCommand::Panes => print_copy(copy::panes()),
        TourCommand::GlobalSearch => print_copy(copy::global_search()),
        TourCommand::VerticalTabs => print_copy(copy::vertical_tabs()),
        TourCommand::Terminal => print_copy(copy::terminal()),
        TourCommand::Coding => print_copy(copy::coding()),
        TourCommand::Agents => print_copy(copy::agents()),
        TourCommand::Knowledge => print_copy(copy::knowledge()),
        TourCommand::Cleanup => print_copy(copy::cleanup()),
    }
}

fn print_copy(text: String) -> Result<(), ControlError> {
    println!("{text}");
    Ok(())
}
