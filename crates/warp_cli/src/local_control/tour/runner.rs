//! Interactive `warpctrl tour run` state machine.
//!
//! Drives the full guided tour deterministically from a terminal: numbered
//! stdin menus, surface demos in a dedicated tour pane, and an opt-in agent
//! escape hatch that stages — but never submits — a prefilled prompt.
use std::collections::HashSet;
use std::io::{BufRead, IsTerminal, Write};

use local_control::discovery::{
    InstanceId, InstanceRecord, discovery_dir, list_instances_from_dir,
};
use local_control::protocol::{
    ActionKind, ControlError, ErrorCode, TabCreateParams, TabType, TargetSelector, ThemeStateResult,
};
use local_control::selection::{InstanceSelector, select_instance};
use serde_json::Value;

use crate::local_control::TourInstanceArgs;
use crate::local_control::tour::composite::{
    finish_session, init_session, open_surface, restore_theme_steps, stage_input,
};
use crate::local_control::tour::copy;
use crate::local_control::tour::invoker::{ActionInvoker, ClientInvoker, pane_target};
use crate::local_control::tour::state::{TourStop, repository_name, surface_name_for_action};

const ENABLEMENT_GUIDANCE: &str = "Warp Control is required for the guided tour. Enable it in Settings > Scripting, then rerun `warpctrl tour run`. If you are developing Warp locally, build with the `warp_control_cli` feature and invoke `--warpctrl tour run`.";

pub(super) fn run_interactive_tour(args: &TourInstanceArgs) -> Result<(), ControlError> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(ControlError::new(
            ErrorCode::InvalidRequest,
            "tour run is interactive and requires a terminal; use `warpctrl tour <stop>` for scripted tour copy",
        ));
    }
    let records = list_instances_from_dir(&discovery_dir());
    if records.is_empty() {
        println!("{ENABLEMENT_GUIDANCE}");
        return Err(ControlError::new(
            ErrorCode::NoInstance,
            "no local Warp control instances were discovered",
        ));
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut output = std::io::stdout();
    let Some(instance) = pick_instance(args, records, &mut input, &mut output)? else {
        return Ok(());
    };
    let invoker = ClientInvoker::new(instance);
    let repo_name = std::env::current_dir()
        .ok()
        .and_then(|dir| repository_name(&dir));
    run_tour_loop(&invoker, &mut input, &mut output, repo_name)
}

fn pick_instance(
    args: &TourInstanceArgs,
    records: Vec<InstanceRecord>,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<Option<InstanceRecord>, ControlError> {
    if let Some(instance_id) = &args.instance {
        let selector = InstanceSelector::Id(InstanceId(instance_id.clone()));
        return select_instance(&records, &selector).map(Some);
    }
    if let Some(pid) = args.pid {
        return select_instance(&records, &InstanceSelector::Pid(pid)).map(Some);
    }
    if records.len() == 1 {
        return Ok(records.into_iter().next());
    }
    let options: Vec<String> = records
        .iter()
        .map(|record| {
            format!(
                "{} (pid {}, {})",
                record.instance_id.0, record.pid, record.channel
            )
        })
        .collect();
    match choose(
        input,
        output,
        "Multiple Warp instances are running — which one should we tour?",
        &options,
    )? {
        Some(index) => Ok(records.into_iter().nth(index)),
        None => Ok(None),
    }
}

/// Runs the tour state machine against arbitrary I/O so tests can script it.
pub(crate) fn run_tour_loop(
    invoker: &dyn ActionInvoker,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    repo_name: Option<String>,
) -> Result<(), ControlError> {
    writeln!(output, "{}", copy::welcome()).map_err(io_error)?;
    let init = init_session(invoker)?;
    for step in init.steps.iter().filter(|step| !step.ok) {
        let message = step
            .error
            .as_ref()
            .map(|error| error.message.as_str())
            .unwrap_or("unknown error");
        writeln!(
            output,
            "  (Heads up: {} didn't work — {message}.)",
            step.step
        )
        .map_err(io_error)?;
    }
    let Some(anchor_pane) = init.anchor.pane_id.clone() else {
        writeln!(
            output,
            "I couldn't find the pane you're running in, so I can't run the tour safely."
        )
        .map_err(io_error)?;
        return Err(ControlError::new(
            ErrorCode::MissingTarget,
            "tour run could not resolve an anchor pane",
        ));
    };
    let Some(tour_pane) = init.tour_pane_id.clone() else {
        writeln!(
            output,
            "I couldn't create the tour split pane, so the tour can't continue. Free up some horizontal space and try again."
        )
        .map_err(io_error)?;
        return Err(ControlError::new(
            ErrorCode::Internal,
            "tour run could not create the tour pane",
        ));
    };
    let available = if init.surfaces.is_empty() {
        None
    } else {
        Some(
            init.surfaces
                .iter()
                .filter(|surface| surface.is_available)
                .map(|surface| surface.name.clone())
                .collect::<HashSet<_>>(),
        )
    };
    let remaining_core = filter_available(TourStop::CORE, &available);
    let mut remaining_topics = filter_available(TourStop::TOPICS, &available);
    if repo_name.is_some() {
        remaining_topics.sort_by_key(|stop| if *stop == TourStop::Coding { 0 } else { 1 });
    }
    if let Some(repo) = &repo_name {
        writeln!(
            output,
            "I see you're working in {repo} — I'll highlight the coding stops when we get there."
        )
        .map_err(io_error)?;
    }
    let mut session = Session {
        invoker,
        input,
        output,
        anchor_pane,
        tour_pane,
        agent_tab: None,
        saved_theme: init.theme,
        needs_theme_restore: false,
        available,
        remaining_core,
        remaining_topics,
    };
    match session.main_menu()? {
        Signal::Continue | Signal::End => session.finish_flow(),
        Signal::Eof => session.best_effort_cleanup(),
    }
}

fn filter_available(stops: &[TourStop], available: &Option<HashSet<String>>) -> Vec<TourStop> {
    stops
        .iter()
        .copied()
        .filter(|stop| {
            stop.surfaces()
                .iter()
                .any(|spec| spec_available(spec.action, available))
        })
        .collect()
}

fn spec_available(action: ActionKind, available: &Option<HashSet<String>>) -> bool {
    let Some(name) = surface_name_for_action(action) else {
        return true;
    };
    match available {
        Some(set) => set.contains(name),
        None => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Continue,
    End,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Core,
    Topics,
    Agent,
    Done,
}

struct Session<'a> {
    invoker: &'a dyn ActionInvoker,
    input: &'a mut dyn BufRead,
    output: &'a mut dyn Write,
    anchor_pane: String,
    tour_pane: String,
    agent_tab: Option<String>,
    saved_theme: Option<ThemeStateResult>,
    needs_theme_restore: bool,
    available: Option<HashSet<String>>,
    remaining_core: Vec<TourStop>,
    remaining_topics: Vec<TourStop>,
}

impl Session<'_> {
    fn main_menu(&mut self) -> Result<Signal, ControlError> {
        loop {
            if self.remaining_core.is_empty() && self.remaining_topics.is_empty() {
                self.say("That's every stop — nice work! 🎉")?;
                return Ok(Signal::End);
            }
            let mut options = Vec::new();
            let mut actions = Vec::new();
            if !self.remaining_core.is_empty() {
                options.push(
                    "Start the core tour (themes, keybindings, panes, search, tabs)".to_owned(),
                );
                actions.push(MenuAction::Core);
            }
            if !self.remaining_topics.is_empty() {
                options.push("Jump to a topic".to_owned());
                actions.push(MenuAction::Topics);
            }
            options.push("Ask Warp's agent a question".to_owned());
            actions.push(MenuAction::Agent);
            options.push("I'm done, thanks!".to_owned());
            actions.push(MenuAction::Done);
            let Some(index) = self.choose("Where to next?", &options)? else {
                return Ok(Signal::Eof);
            };
            let Some(action) = actions.get(index) else {
                continue;
            };
            match action {
                MenuAction::Core => match self.core_tour()? {
                    Signal::Continue => {}
                    signal => return Ok(signal),
                },
                MenuAction::Topics => match self.topic_menu()? {
                    Signal::Continue => {}
                    signal => return Ok(signal),
                },
                MenuAction::Agent => {
                    if self.agent_handoff(None)? == Signal::Eof {
                        return Ok(Signal::Eof);
                    }
                }
                MenuAction::Done => return Ok(Signal::End),
            }
        }
    }

    fn core_tour(&mut self) -> Result<Signal, ControlError> {
        while let Some(stop) = self.remaining_core.first().copied() {
            match self.run_stop(stop)? {
                Signal::Continue => {}
                signal => return Ok(signal),
            }
            self.remaining_core.retain(|remaining| *remaining != stop);
            if self.remaining_core.is_empty() {
                self.say("That's the core tour! 🎉")?;
                break;
            }
            let options = vec![
                "Next stop →".to_owned(),
                "Back to the menu".to_owned(),
                "End the tour".to_owned(),
            ];
            match self.choose("Ready for more?", &options)? {
                None => return Ok(Signal::Eof),
                Some(0) => {}
                Some(1) => break,
                Some(_) => return Ok(Signal::End),
            }
        }
        Ok(Signal::Continue)
    }

    fn topic_menu(&mut self) -> Result<Signal, ControlError> {
        loop {
            if self.remaining_topics.is_empty() {
                return Ok(Signal::Continue);
            }
            let mut options: Vec<String> = self
                .remaining_topics
                .iter()
                .map(|stop| stop.title().to_owned())
                .collect();
            options.push("Back to the main menu".to_owned());
            let Some(index) = self.choose("Pick a topic:", &options)? else {
                return Ok(Signal::Eof);
            };
            let Some(stop) = self.remaining_topics.get(index).copied() else {
                return Ok(Signal::Continue);
            };
            match self.run_stop(stop)? {
                Signal::Continue => {}
                signal => return Ok(signal),
            }
            self.remaining_topics.retain(|remaining| *remaining != stop);
        }
    }

    fn run_stop(&mut self, stop: TourStop) -> Result<Signal, ControlError> {
        self.say(&stop.copy())?;
        if stop == TourStop::Themes && self.saved_theme.is_some() {
            self.needs_theme_restore = true;
        }
        for spec in stop.surfaces() {
            if !spec_available(spec.action, &self.available) {
                continue;
            }
            let (step, result) = open_surface(self.invoker, spec, &self.tour_pane);
            if let Err(error) = result {
                self.say(&format!(
                    "  (I couldn't open {step} — {} — skipping that demo.)",
                    error.message
                ))?;
                if let (Some(name), Some(set)) = (
                    surface_name_for_action(spec.action),
                    self.available.as_mut(),
                ) {
                    set.remove(name);
                }
            }
        }
        if let Err(error) = self.focus_anchor() {
            self.say(&format!(
                "I lost track of our anchor pane ({}). Let's wrap up.",
                error.message
            ))?;
            return Ok(Signal::End);
        }
        let signal = self.task_loop(stop)?;
        if stop == TourStop::Themes {
            self.restore_theme()?;
        }
        Ok(signal)
    }

    fn task_loop(&mut self, stop: TourStop) -> Result<Signal, ControlError> {
        self.say(&format!("Your turn: {}", stop.task()))?;
        loop {
            let options = vec![
                "Done! ✅".to_owned(),
                "I need a hint 💡".to_owned(),
                "Skip this one".to_owned(),
                "Ask Warp's agent".to_owned(),
                "End the tour".to_owned(),
            ];
            match self.choose("How's it going?", &options)? {
                None => return Ok(Signal::Eof),
                Some(0) | Some(2) => return Ok(Signal::Continue),
                Some(1) => self.say(&format!("Hint: {}", stop.hint()))?,
                Some(3) => {
                    if self.agent_handoff(Some(stop))? == Signal::Eof {
                        return Ok(Signal::Eof);
                    }
                }
                Some(_) => return Ok(Signal::End),
            }
        }
    }

    fn agent_handoff(&mut self, stop: Option<TourStop>) -> Result<Signal, ControlError> {
        self.say("What would you like to ask Warp's agent? (one line)")?;
        let Some(question) = self.read_line()? else {
            return Ok(Signal::Eof);
        };
        let question = question.trim().to_owned();
        if question.is_empty() {
            self.say("No question staged — back to the tour.")?;
            return Ok(Signal::Continue);
        }
        if self.agent_tab.is_none() {
            match self.create_agent_tab() {
                Ok(tab_id) => self.agent_tab = Some(tab_id),
                Err(error) => {
                    self.say(&format!(
                        "I couldn't open an agent tab ({}) — let's keep touring.",
                        error.message
                    ))?;
                    return Ok(Signal::Continue);
                }
            }
        }
        let Some(agent_tab) = self.agent_tab.clone() else {
            return Ok(Signal::Continue);
        };
        let context = match stop {
            Some(stop) => format!("Current tour stop: {}.", stop.title()),
            None => "I'm between tour stops.".to_owned(),
        };
        let prompt = format!(
            "Warp tour question: {question}\n\nContext: I'm taking the interactive Warp tour (warpctrl tour run). {context} The tour demo pane is {}, and the tour terminal (anchor) pane is {} — please don't close them or change my settings.",
            self.tour_pane, self.anchor_pane
        );
        if let Err(error) = stage_input(self.invoker, &agent_tab, prompt) {
            self.say(&format!(
                "I couldn't stage the question ({}) — let's keep touring.",
                error.message
            ))?;
            return Ok(Signal::Continue);
        }
        self.say(
            "I staged your question in an agent tab. Review or edit it there, then press Enter to send it — nothing is sent (or billed) until you do.",
        )?;
        self.say("Press Enter here when you're ready to resume the tour.")?;
        if self.read_line()?.is_none() {
            return Ok(Signal::Eof);
        }
        if let Err(error) = self.focus_anchor() {
            self.say(&format!(
                "(Couldn't refocus the tour pane — {}.)",
                error.message
            ))?;
        }
        Ok(Signal::Continue)
    }

    fn create_agent_tab(&mut self) -> Result<String, ControlError> {
        let params = serde_json::to_value(TabCreateParams {
            tab_type: Some(TabType::Agent),
            shell: None,
        })
        .map_err(|err| {
            ControlError::with_details(
                ErrorCode::InvalidParams,
                "failed to serialize agent tab parameters",
                err.to_string(),
            )
        })?;
        let data = self
            .invoker
            .invoke(ActionKind::TabCreate, params, TargetSelector::default())?;
        data.get("tab")
            .and_then(|tab| tab.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::Internal,
                    "tab.create did not return the new agent tab id",
                )
            })
    }

    fn finish_flow(&mut self) -> Result<(), ControlError> {
        self.say(&copy::cleanup())?;
        self.restore_theme()?;
        let options = vec![
            "Clean up tour panes/tabs 🧹".to_owned(),
            "Leave them open, I'm done".to_owned(),
        ];
        match self.choose("One last thing:", &options)? {
            None => return self.best_effort_cleanup(),
            Some(0) => {
                self.say("Closing up — confirm any normal Warp close prompts that appear.")?;
                let tour_tabs: Vec<String> = self.agent_tab.iter().cloned().collect();
                let result = finish_session(self.invoker, Some(&self.tour_pane), &tour_tabs, None);
                for step in &result.steps {
                    if step.ok {
                        self.say(&format!("  ✓ {}", step.step))?;
                    } else {
                        let message = step
                            .error
                            .as_ref()
                            .map(|error| error.message.as_str())
                            .unwrap_or("unknown error");
                        self.say(&format!(
                            "  ✗ {} ({message}) — it may still be open.",
                            step.step
                        ))?;
                    }
                }
            }
            Some(_) => self.say("No problem — I'll leave everything as is.")?,
        }
        Ok(())
    }

    fn best_effort_cleanup(&mut self) -> Result<(), ControlError> {
        if self.needs_theme_restore {
            self.restore_theme()?;
        }
        let agent_note = match &self.agent_tab {
            Some(tab) => format!(" and agent tab {tab}"),
            None => String::new(),
        };
        self.say(&format!(
            "\nTour ended. Still open: tour pane {}{agent_note}. Close them like any other pane/tab when you're done.",
            self.tour_pane
        ))?;
        Ok(())
    }

    fn restore_theme(&mut self) -> Result<(), ControlError> {
        if !self.needs_theme_restore {
            return Ok(());
        }
        let Some(theme) = self.saved_theme.clone() else {
            return Ok(());
        };
        let steps = restore_theme_steps(self.invoker, &theme);
        if steps.iter().any(|step| !step.ok) {
            self.say("(I couldn't fully restore your theme — check Settings > Appearance.)")?;
        } else {
            self.say("(Your original theme is restored.)")?;
        }
        self.needs_theme_restore = false;
        Ok(())
    }

    fn focus_anchor(&mut self) -> Result<(), ControlError> {
        self.invoker
            .invoke(
                ActionKind::PaneFocus,
                serde_json::Value::Object(Default::default()),
                pane_target(&self.anchor_pane),
            )
            .map(|_| ())
    }

    fn say(&mut self, text: &str) -> Result<(), ControlError> {
        writeln!(self.output, "{text}").map_err(io_error)
    }

    fn read_line(&mut self) -> Result<Option<String>, ControlError> {
        let mut line = String::new();
        let read = self.input.read_line(&mut line).map_err(io_error)?;
        if read == 0 { Ok(None) } else { Ok(Some(line)) }
    }

    fn choose(&mut self, header: &str, options: &[String]) -> Result<Option<usize>, ControlError> {
        choose(&mut *self.input, &mut *self.output, header, options)
    }
}

fn choose(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    header: &str,
    options: &[String],
) -> Result<Option<usize>, ControlError> {
    loop {
        writeln!(output, "\n{header}").map_err(io_error)?;
        for (index, option) in options.iter().enumerate() {
            writeln!(output, "  {}) {option}", index + 1).map_err(io_error)?;
        }
        write!(output, "> ").map_err(io_error)?;
        output.flush().map_err(io_error)?;
        let mut line = String::new();
        if input.read_line(&mut line).map_err(io_error)? == 0 {
            return Ok(None);
        }
        match line.trim().parse::<usize>() {
            Ok(choice) if (1..=options.len()).contains(&choice) => return Ok(Some(choice - 1)),
            _ => writeln!(
                output,
                "Please enter a number between 1 and {}.",
                options.len()
            )
            .map_err(io_error)?,
        }
    }
}

fn io_error(error: std::io::Error) -> ControlError {
    ControlError::with_details(
        ErrorCode::Internal,
        "failed to read or write tour terminal io",
        error.to_string(),
    )
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
