use std::io::{self, stdout, Stdout, Write};
use std::time::Duration;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use crossterm::style::Print;
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, queue};
use warpui_core::{App, AppContext, Entity, ModelHandle};
use warpui_tui::elements::{TuiColumn, TuiContainer, TuiElement, TuiEventHandler, TuiText};
use warpui_tui::{
    crossterm_event_to_warp_event, TuiDispatchEventResult, TuiFrame, TuiPresenter, TuiSize,
    TuiView,
};

struct TodoItem {
    label: String,
    done: bool,
}

struct TodoModel {
    title: String,
    items: Vec<TodoItem>,
    selected_index: usize,
    should_quit: bool,
}

impl TodoModel {
    fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.items.len();
        }
    }

    fn select_previous(&mut self) {
        if !self.items.is_empty() {
            self.selected_index = (self.selected_index + self.items.len() - 1) % self.items.len();
        }
    }

    fn toggle_selected(&mut self) {
        if let Some(item) = self.items.get_mut(self.selected_index) {
            item.done = !item.done;
        }
    }

    fn quit(&mut self) {
        self.should_quit = true;
    }
}

impl Entity for TodoModel {
    type Event = ();
}

struct TodoView {
    model: ModelHandle<TodoModel>,
}

impl Entity for TodoView {
    type Event = ();
}

impl TuiView for TodoView {
    type RenderOutput = Box<dyn TuiElement>;

    fn ui_name() -> &'static str {
        "InteractiveTodoView"
    }

    fn render_tui(&self, app: &AppContext) -> Self::RenderOutput {
        let lines = self.model.read(app, |model, _| {
            let mut lines = vec![model.title.clone(), String::new()];
            lines.extend(model.items.iter().enumerate().map(|(index, item)| {
                let marker = if index == model.selected_index {
                    ">"
                } else {
                    " "
                };
                let checkbox = if item.done { "[x]" } else { "[ ]" };
                format!("{marker} {checkbox} {}", item.label)
            }));
            lines.push(String::new());
            lines.push("↑/↓ or j/k: move   space: toggle   q/esc: quit".to_owned());
            lines
        });

        let children = lines
            .into_iter()
            .map(|line| Box::new(TuiText::new(line)) as Box<dyn TuiElement>);
        let model = self.model.clone();

        Box::new(
            TuiEventHandler::new(TuiContainer::new(TuiColumn::new(children)).with_border())
                .on_key_down(move |ctx, _, keystroke| {
                    let model = model.clone();
                    match keystroke.key.as_str() {
                        "up" | "k" if keystroke.is_unmodified() => {
                            ctx.dispatch_app_update(move |app| {
                                model.update(app, |model, ctx| {
                                    model.select_previous();
                                    ctx.notify();
                                });
                            });
                            TuiDispatchEventResult::StopPropagation
                        }
                        "down" | "j" if keystroke.is_unmodified() => {
                            ctx.dispatch_app_update(move |app| {
                                model.update(app, |model, ctx| {
                                    model.select_next();
                                    ctx.notify();
                                });
                            });
                            TuiDispatchEventResult::StopPropagation
                        }
                        " " | "enter" if keystroke.is_unmodified() => {
                            ctx.dispatch_app_update(move |app| {
                                model.update(app, |model, ctx| {
                                    model.toggle_selected();
                                    ctx.notify();
                                });
                            });
                            TuiDispatchEventResult::StopPropagation
                        }
                        "escape" | "q" if keystroke.is_unmodified() => {
                            ctx.dispatch_app_update(move |app| {
                                model.update(app, |model, _| {
                                    model.quit();
                                });
                            });
                            TuiDispatchEventResult::StopPropagation
                        }
                        _ => TuiDispatchEventResult::PropagateToParent,
                    }
                }),
        )
    }
}

struct TerminalSession {
    stdout: Stdout,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;

        let mut stdout = stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }

        Ok(Self { stdout })
    }

    fn size(&self) -> io::Result<TuiSize> {
        let (width, height) = terminal::size()?;
        Ok(TuiSize::new(width.max(1), height.max(1)))
    }

    fn draw(&mut self, frame: &TuiFrame) -> io::Result<()> {
        queue!(self.stdout, MoveTo(0, 0), Clear(ClearType::All))?;

        for (row, line) in frame.buffer.lines().into_iter().enumerate() {
            let Ok(row) = u16::try_from(row) else {
                break;
            };
            queue!(self.stdout, MoveTo(0, row), Print(line))?;
        }

        self.stdout.flush()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = execute!(self.stdout, Show, DisableMouseCapture, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn main() -> io::Result<()> {
    let terminal = TerminalSession::enter()?;

    App::test((), |mut app| async move {
        let mut terminal = terminal;
        let mut presenter = TuiPresenter::new();
        let model = app.add_model(|_| TodoModel {
            title: "warpui_tui interactive example".to_owned(),
            items: vec![
                TodoItem {
                    label: "reuse warpui_core models".to_owned(),
                    done: true,
                },
                TodoItem {
                    label: "render through a TuiView".to_owned(),
                    done: false,
                },
                TodoItem {
                    label: "paint into a terminal buffer".to_owned(),
                    done: false,
                },
            ],
            selected_index: 1,
            should_quit: false,
        });

        let (_, root_view) = app.add_tui_window(|_| TodoView {
            model: model.clone(),
        });

        loop {
            let size = terminal.size()?;
            let frame = app.read(|ctx| {
                root_view.read(ctx, |view, ctx| presenter.render_view(view, ctx, size))
            });
            terminal.draw(&frame)?;

            if event::poll(Duration::from_millis(250))? {
                if let Some(event) = crossterm_event_to_warp_event(event::read()?) {
                    presenter.dispatch_event(&event, &mut app);
                }
            }

            if model.read(&app, |model, _| model.should_quit) {
                break;
            }
        }

        Ok(())
    })
}