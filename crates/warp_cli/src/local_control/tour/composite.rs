//! Composite tour commands that collapse a full tour phase into one
//! `warpctrl` invocation: `tour init`, `tour stop <name>`, and `tour finish`.
//!
//! Each composite is pure client-side orchestration over the existing action
//! catalog. Every underlying action is individually credentialed and
//! dispatched; per-step results are reported instead of aborting on the first
//! recoverable failure.
use std::collections::HashSet;

use local_control::protocol::{
    ActionKind, ActiveTargetChain, BooleanValueParams, ControlError, Direction, DirectionParams,
    ErrorCode, KeybindingListResult, KeybindingSummary, PageQueryParams, QueryParams,
    SurfaceListResult, SurfaceSummary, TabCloseMode, TabCloseParams, TargetSelector, TextParams,
    ThemeNameParams, ThemeStateResult,
};
use serde::Serialize;

use crate::agent::OutputFormat;
use crate::local_control::commands::resolve_instance;
use crate::local_control::output::{write_json, write_json_line};
use crate::local_control::tour::copy;
use crate::local_control::tour::invoker::{ActionInvoker, ClientInvoker, pane_target, tab_target};
use crate::local_control::tour::state::{SurfaceOpenSpec, TourStop};
use crate::local_control::{TargetArgs, TourFinishArgs, TourInstanceArgs, TourStopArgs};

/// Outcome of one action dispatched within a composite tour command.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct TourStepResult {
    pub step: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlError>,
}

impl TourStepResult {
    fn from_result(
        step: impl Into<String>,
        result: &Result<serde_json::Value, ControlError>,
    ) -> Self {
        Self {
            step: step.into(),
            ok: result.is_ok(),
            error: result.as_ref().err().cloned(),
        }
    }

    fn failure(step: impl Into<String>, error: ControlError) -> Self {
        Self {
            step: step.into(),
            ok: false,
            error: Some(error),
        }
    }
}

/// Structured result of `warpctrl tour init`.
#[derive(Debug, Serialize)]
pub(crate) struct TourInitResult {
    pub anchor: ActiveTargetChain,
    pub tour_pane_id: Option<String>,
    pub surfaces: Vec<SurfaceSummary>,
    pub theme: Option<ThemeStateResult>,
    pub steps: Vec<TourStepResult>,
}

/// Structured result of `warpctrl tour stop <name>`.
#[derive(Debug, Serialize)]
pub(crate) struct TourStopResult {
    pub stop: &'static str,
    pub copy: String,
    pub steps: Vec<TourStepResult>,
    pub keybindings: Vec<KeybindingSummary>,
    pub anchor_refocused: bool,
}

/// Structured result of `warpctrl tour finish`.
#[derive(Debug, Serialize)]
pub(crate) struct TourFinishResult {
    pub copy: String,
    pub steps: Vec<TourStepResult>,
}

/// Starts a tour session: anchor chain, surface availability, saved theme
/// state, and a freshly split tour pane.
///
/// Returns `Err` only when the session cannot start at all (for example when
/// `app.active` fails); recoverable failures are reported as steps.
pub(crate) fn init_session(invoker: &dyn ActionInvoker) -> Result<TourInitResult, ControlError> {
    let mut steps = Vec::new();
    let active = invoker.invoke(
        ActionKind::AppActive,
        empty_params(),
        TargetSelector::default(),
    )?;
    steps.push(TourStepResult::from_result(
        ActionKind::AppActive.as_str(),
        &Ok(active.clone()),
    ));
    let anchor: ActiveTargetChain = active
        .get("active")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|err| {
            ControlError::with_details(
                ErrorCode::Internal,
                "tour init could not decode the active target chain",
                err.to_string(),
            )
        })?
        .unwrap_or(ActiveTargetChain {
            instance_id: None,
            window_id: None,
            tab_id: None,
            pane_id: None,
            session_id: None,
        });

    let surfaces_result = invoker.invoke(
        ActionKind::SurfaceList,
        empty_params(),
        TargetSelector::default(),
    );
    steps.push(TourStepResult::from_result(
        ActionKind::SurfaceList.as_str(),
        &surfaces_result,
    ));
    let surfaces = surfaces_result
        .ok()
        .and_then(|data| serde_json::from_value::<SurfaceListResult>(data).ok())
        .map(|result| result.surfaces)
        .unwrap_or_default();

    let theme_result = invoker.invoke(
        ActionKind::ThemeGet,
        empty_params(),
        TargetSelector::default(),
    );
    steps.push(TourStepResult::from_result(
        ActionKind::ThemeGet.as_str(),
        &theme_result,
    ));
    let theme = theme_result
        .ok()
        .and_then(|data| serde_json::from_value::<ThemeStateResult>(data).ok());

    let tour_pane_id = create_tour_pane(invoker, &anchor, &mut steps);
    Ok(TourInitResult {
        anchor,
        tour_pane_id,
        surfaces,
        theme,
        steps,
    })
}

/// Splits a tour pane to the right of the anchor and identifies it by diffing
/// `pane.list` before and after the split.
fn create_tour_pane(
    invoker: &dyn ActionInvoker,
    anchor: &ActiveTargetChain,
    steps: &mut Vec<TourStepResult>,
) -> Option<String> {
    let split_step = ActionKind::PaneSplit.as_str();
    let Some(anchor_pane) = anchor.pane_id.as_deref() else {
        steps.push(TourStepResult::failure(
            split_step,
            ControlError::new(
                ErrorCode::MissingTarget,
                "tour init could not resolve an anchor pane to split",
            ),
        ));
        return None;
    };
    let before = match list_pane_ids(invoker) {
        Ok(panes) => panes,
        Err(error) => {
            steps.push(TourStepResult::failure(
                ActionKind::PaneList.as_str(),
                error,
            ));
            return None;
        }
    };
    let params = match to_params(DirectionParams {
        direction: Direction::Right,
    }) {
        Ok(params) => params,
        Err(error) => {
            steps.push(TourStepResult::failure(split_step, error));
            return None;
        }
    };
    let split = invoker.invoke(ActionKind::PaneSplit, params, pane_target(anchor_pane));
    steps.push(TourStepResult::from_result(split_step, &split));
    if split.is_err() {
        return None;
    }
    let after = match list_pane_ids(invoker) {
        Ok(panes) => panes,
        Err(error) => {
            steps.push(TourStepResult::failure(
                ActionKind::PaneList.as_str(),
                error,
            ));
            return None;
        }
    };
    let before: HashSet<String> = before.into_iter().collect();
    let mut new_panes = after.into_iter().filter(|id| !before.contains(id));
    let tour_pane = new_panes.next();
    if tour_pane.is_none() || new_panes.next().is_some() {
        steps.push(TourStepResult::failure(
            split_step,
            ControlError::new(
                ErrorCode::Internal,
                "tour init could not uniquely identify the new tour pane",
            ),
        ));
        return None;
    }
    let refocus = invoker.invoke(
        ActionKind::PaneFocus,
        empty_params(),
        pane_target(anchor_pane),
    );
    steps.push(TourStepResult::from_result(
        ActionKind::PaneFocus.as_str(),
        &refocus,
    ));
    tour_pane
}

fn list_pane_ids(invoker: &dyn ActionInvoker) -> Result<Vec<String>, ControlError> {
    let data = invoker.invoke(
        ActionKind::PaneList,
        empty_params(),
        TargetSelector::default(),
    )?;
    let panes = data
        .get("panes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::Internal,
                "pane.list returned malformed pane metadata",
            )
        })?;
    Ok(panes
        .iter()
        .filter_map(|pane| pane.get("pane_id").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect())
}

/// Opens one stop surface in the tour pane, returning the step name and result.
pub(crate) fn open_surface(
    invoker: &dyn ActionInvoker,
    spec: &SurfaceOpenSpec,
    tour_pane: &str,
) -> (String, Result<serde_json::Value, ControlError>) {
    let step = spec.action.as_str().to_owned();
    let params = match spec.action {
        ActionKind::SurfaceSettingsOpen => to_params(PageQueryParams {
            page: None,
            query: spec.query.map(str::to_owned),
        }),
        ActionKind::SurfaceCommandPaletteOpen | ActionKind::SurfaceCommandSearchOpen => {
            to_params(QueryParams {
                query: spec.query.map(str::to_owned),
            })
        }
        _ => Ok(empty_params()),
    };
    let result = match params {
        Ok(params) => invoker.invoke(spec.action, params, pane_target(tour_pane)),
        Err(error) => Err(error),
    };
    (step, result)
}

/// Performs one full tour stop: surface opens, keybinding resolution, and
/// anchor refocus, with the stop's copy embedded in the result.
pub(crate) fn run_stop_steps(
    invoker: &dyn ActionInvoker,
    stop: TourStop,
    tour_pane: &str,
    anchor_pane: &str,
) -> TourStopResult {
    let mut steps = Vec::new();
    for spec in stop.surfaces() {
        let (step, result) = open_surface(invoker, spec, tour_pane);
        steps.push(TourStepResult::from_result(step, &result));
    }
    let (keybindings, keybinding_step) = resolve_stop_keybindings(invoker, stop);
    if let Some(step) = keybinding_step {
        steps.push(step);
    }
    let refocus = invoker.invoke(
        ActionKind::PaneFocus,
        empty_params(),
        pane_target(anchor_pane),
    );
    let anchor_refocused = refocus.is_ok();
    steps.push(TourStepResult::from_result(
        ActionKind::PaneFocus.as_str(),
        &refocus,
    ));
    TourStopResult {
        stop: stop.cli_name(),
        copy: stop.copy(),
        steps,
        keybindings,
        anchor_refocused,
    }
}

/// Resolves keybindings relevant to a stop by filtering `keybinding.list`.
fn resolve_stop_keybindings(
    invoker: &dyn ActionInvoker,
    stop: TourStop,
) -> (Vec<KeybindingSummary>, Option<TourStepResult>) {
    let needles = stop.keybinding_needles();
    if needles.is_empty() {
        return (Vec::new(), None);
    }
    let result = invoker.invoke(
        ActionKind::KeybindingList,
        empty_params(),
        TargetSelector::default(),
    );
    let step = TourStepResult::from_result(ActionKind::KeybindingList.as_str(), &result);
    let keybindings = result
        .ok()
        .and_then(|data| serde_json::from_value::<KeybindingListResult>(data).ok())
        .map(|result| result.keybindings)
        .unwrap_or_default()
        .into_iter()
        .filter(|keybinding| {
            let haystack = format!("{} {}", keybinding.name, keybinding.description).to_lowercase();
            needles.iter().any(|needle| haystack.contains(needle))
        })
        .collect();
    (keybindings, Some(step))
}

/// Restores saved theme state through the theme catalog actions.
pub(crate) fn restore_theme_steps(
    invoker: &dyn ActionInvoker,
    theme: &ThemeStateResult,
) -> Vec<TourStepResult> {
    let mut steps = Vec::new();
    push_invoke(
        invoker,
        &mut steps,
        ActionKind::ThemeSystemSet,
        to_params(BooleanValueParams {
            value: theme.follow_system_theme,
        }),
    );
    if let Some(light_theme) = &theme.light_theme {
        push_invoke(
            invoker,
            &mut steps,
            ActionKind::ThemeLightSet,
            to_params(ThemeNameParams {
                theme_name: light_theme.clone(),
            }),
        );
    }
    if let Some(dark_theme) = &theme.dark_theme {
        push_invoke(
            invoker,
            &mut steps,
            ActionKind::ThemeDarkSet,
            to_params(ThemeNameParams {
                theme_name: dark_theme.clone(),
            }),
        );
    }
    if !theme.follow_system_theme {
        push_invoke(
            invoker,
            &mut steps,
            ActionKind::ThemeSet,
            to_params(ThemeNameParams {
                theme_name: theme.name.clone(),
            }),
        );
    }
    steps
}

/// Finishes a tour session: theme restore plus closing exactly the given
/// tour-created targets.
pub(crate) fn finish_session(
    invoker: &dyn ActionInvoker,
    tour_pane: Option<&str>,
    tour_tabs: &[String],
    theme: Option<&ThemeStateResult>,
) -> TourFinishResult {
    let mut steps = Vec::new();
    if let Some(theme) = theme {
        steps.extend(restore_theme_steps(invoker, theme));
    }
    if let Some(tour_pane) = tour_pane {
        let result = invoker.invoke(
            ActionKind::PaneClose,
            empty_params(),
            pane_target(tour_pane),
        );
        steps.push(TourStepResult::from_result(
            ActionKind::PaneClose.as_str(),
            &result,
        ));
    }
    for tab in tour_tabs {
        let result = match to_params(TabCloseParams {
            mode: TabCloseMode::Target,
        }) {
            Ok(params) => invoker.invoke(ActionKind::TabClose, params, tab_target(tab)),
            Err(error) => Err(error),
        };
        steps.push(TourStepResult::from_result(
            ActionKind::TabClose.as_str(),
            &result,
        ));
    }
    TourFinishResult {
        copy: copy::cleanup(),
        steps,
    }
}

/// Stages text in a target tab's input without submitting it.
pub(crate) fn stage_input(
    invoker: &dyn ActionInvoker,
    tab_id: &str,
    text: String,
) -> Result<serde_json::Value, ControlError> {
    invoker.invoke(
        ActionKind::InputInsert,
        to_params(TextParams { text })?,
        tab_target(tab_id),
    )
}

pub(super) fn run_init_command(
    args: &TourInstanceArgs,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    let invoker = client_invoker(args)?;
    let result = init_session(&invoker)?;
    emit_init(&result, output_format)?;
    if result.tour_pane_id.is_none() {
        return Err(first_step_error(
            &result.steps,
            "tour init could not create the tour pane",
        ));
    }
    Ok(())
}

pub(super) fn run_stop_command(
    args: &TourStopArgs,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    let invoker = client_invoker(&args.instance)?;
    let result = run_stop_steps(&invoker, args.stop, &args.tour_pane, &args.anchor_pane);
    emit_stop(&result, output_format)?;
    if !result.anchor_refocused {
        return Err(first_step_error(
            &result.steps,
            "tour stop could not refocus the anchor pane",
        ));
    }
    Ok(())
}

pub(super) fn run_finish_command(
    args: &TourFinishArgs,
    output_format: OutputFormat,
) -> Result<(), ControlError> {
    let theme = args
        .restore_theme
        .as_deref()
        .map(serde_json::from_str::<ThemeStateResult>)
        .transpose()
        .map_err(|err| {
            ControlError::with_details(
                ErrorCode::InvalidParams,
                "tour finish could not decode --restore-theme JSON",
                err.to_string(),
            )
        })?;
    let invoker = client_invoker(&args.instance)?;
    let result = finish_session(
        &invoker,
        args.tour_pane.as_deref(),
        &args.tour_tabs,
        theme.as_ref(),
    );
    emit_finish(&result, output_format)?;
    if !result.steps.is_empty() && result.steps.iter().all(|step| !step.ok) {
        return Err(first_step_error(
            &result.steps,
            "tour finish could not perform any cleanup step",
        ));
    }
    Ok(())
}

fn client_invoker(args: &TourInstanceArgs) -> Result<ClientInvoker, ControlError> {
    let target_args = TargetArgs {
        instance: args.instance.clone(),
        pid: args.pid,
        ..Default::default()
    };
    Ok(ClientInvoker::new(resolve_instance(&target_args)?))
}

fn first_step_error(steps: &[TourStepResult], fallback: &str) -> ControlError {
    steps
        .iter()
        .find_map(|step| step.error.clone())
        .unwrap_or_else(|| ControlError::new(ErrorCode::Internal, fallback))
}

fn emit_init(result: &TourInitResult, output_format: OutputFormat) -> Result<(), ControlError> {
    match output_format {
        OutputFormat::Json => write_json(result),
        OutputFormat::Ndjson => write_json_line(result),
        OutputFormat::Pretty | OutputFormat::Text => {
            println!(
                "anchor: window {} / tab {} / pane {}",
                display_id(result.anchor.window_id.as_deref()),
                display_id(result.anchor.tab_id.as_deref()),
                display_id(result.anchor.pane_id.as_deref()),
            );
            println!("tour pane: {}", display_id(result.tour_pane_id.as_deref()));
            let available = result
                .surfaces
                .iter()
                .filter(|surface| surface.is_available)
                .count();
            println!(
                "surfaces: {available}/{} available, theme saved: {}",
                result.surfaces.len(),
                if result.theme.is_some() { "yes" } else { "no" }
            );
            print_steps(&result.steps);
            Ok(())
        }
    }
}

fn emit_stop(result: &TourStopResult, output_format: OutputFormat) -> Result<(), ControlError> {
    match output_format {
        OutputFormat::Json => write_json(result),
        OutputFormat::Ndjson => write_json_line(result),
        OutputFormat::Pretty | OutputFormat::Text => {
            println!("{}", result.copy);
            for keybinding in &result.keybindings {
                let keystroke = keybinding.keystroke.as_deref().unwrap_or("unbound");
                println!("  ⌨ {} — {keystroke}", keybinding.description);
            }
            print_steps(&result.steps);
            Ok(())
        }
    }
}

fn emit_finish(result: &TourFinishResult, output_format: OutputFormat) -> Result<(), ControlError> {
    match output_format {
        OutputFormat::Json => write_json(result),
        OutputFormat::Ndjson => write_json_line(result),
        OutputFormat::Pretty | OutputFormat::Text => {
            println!("{}", result.copy);
            print_steps(&result.steps);
            Ok(())
        }
    }
}

fn print_steps(steps: &[TourStepResult]) {
    for step in steps {
        if step.ok {
            println!("  ✓ {}", step.step);
        } else {
            let message = step
                .error
                .as_ref()
                .map(|error| error.message.as_str())
                .unwrap_or("unknown error");
            println!("  ✗ {} ({message})", step.step);
        }
    }
}

fn push_invoke(
    invoker: &dyn ActionInvoker,
    steps: &mut Vec<TourStepResult>,
    action: ActionKind,
    params: Result<serde_json::Value, ControlError>,
) {
    let result = match params {
        Ok(params) => invoker.invoke(action, params, TargetSelector::default()),
        Err(error) => Err(error),
    };
    steps.push(TourStepResult::from_result(action.as_str(), &result));
}

fn display_id(id: Option<&str>) -> &str {
    id.unwrap_or("<unknown>")
}

fn empty_params() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

fn to_params<T: Serialize>(params: T) -> Result<serde_json::Value, ControlError> {
    serde_json::to_value(params).map_err(|err| {
        ControlError::with_details(
            ErrorCode::InvalidParams,
            "failed to serialize tour action parameters",
            err.to_string(),
        )
    })
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod tests;
