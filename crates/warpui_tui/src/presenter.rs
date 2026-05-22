use warpui_core::{App, AppContext, Event};

use crate::elements::TuiElement;
use crate::{
    TuiBuffer, TuiConstraint, TuiEventContext, TuiEventDispatchResult, TuiRect, TuiSize, TuiView,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiFrame {
    pub buffer: TuiBuffer,
    pub cursor_position: Option<(u16, u16)>,
}

pub struct TuiPresenter {
    frame_count: usize,
    root: Option<Box<dyn TuiElement>>,
    root_area: TuiRect,
}

impl TuiPresenter {
    pub fn new() -> Self {
        Self {
            frame_count: 0,
            root: None,
            root_area: TuiRect::default(),
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    pub fn render_view(
        &mut self,
        view: &impl TuiView<RenderOutput = Box<dyn TuiElement>>,
        app: &AppContext,
        size: TuiSize,
    ) -> TuiFrame {
        let mut root = view.render_tui(app);
        root.layout(TuiConstraint::tight(size));

        let mut buffer = TuiBuffer::new(size);
        let area = TuiRect::new(0, 0, size.width, size.height);
        root.render(area, &mut buffer);
        let cursor_position = root.cursor_position(area);
        self.root = Some(root);
        self.root_area = area;
        self.frame_count += 1;
        TuiFrame {
            buffer,
            cursor_position,
        }
    }

    pub fn dispatch_event(&mut self, event: &Event, app: &mut App) -> TuiEventDispatchResult {
        let Some(mut root) = self.root.take() else {
            return TuiEventDispatchResult { handled: false };
        };

        let mut event_ctx = TuiEventContext::default();
        let handled = app.read(|ctx| root.dispatch_event(event, &mut event_ctx, ctx));
        self.root = Some(root);

        for update in event_ctx.take_updates() {
            update(app);
        }

        TuiEventDispatchResult { handled }
    }
}

impl Default for TuiPresenter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use warpui_core::event::KeyEventDetails;
    use warpui_core::keymap::Keystroke;
    use warpui_core::{App, Entity, Event, ModelHandle};

    use super::*;
    use crate::elements::{TuiContainer, TuiEventHandler, TuiText};
    use crate::TuiDispatchEventResult;

    struct GreetingModel {
        greeting: String,
    }

    impl Entity for GreetingModel {
        type Event = ();
    }

    struct GreetingView {
        model: ModelHandle<GreetingModel>,
    }

    impl Entity for GreetingView {
        type Event = ();
    }

    impl TuiView for GreetingView {
        type RenderOutput = Box<dyn crate::elements::TuiElement>;
        fn ui_name() -> &'static str {
            "GreetingView"
        }

        fn render_tui(&self, app: &AppContext) -> Box<dyn crate::elements::TuiElement> {
            let greeting = self.model.read(app, |model, _| model.greeting.clone());
            Box::new(TuiContainer::new(TuiText::new(greeting)).with_border())
        }
    }

    struct EventHandlingView {
        model: ModelHandle<GreetingModel>,
    }

    impl Entity for EventHandlingView {
        type Event = ();
    }

    impl TuiView for EventHandlingView {
        type RenderOutput = Box<dyn crate::elements::TuiElement>;

        fn ui_name() -> &'static str {
            "EventHandlingView"
        }

        fn render_tui(&self, _: &AppContext) -> Box<dyn crate::elements::TuiElement> {
            let model = self.model.clone();
            Box::new(
                TuiEventHandler::new(TuiText::new("press enter")).on_key_down(
                    move |ctx, _, keystroke| {
                        if keystroke.is_unmodified_enter() {
                            let model = model.clone();
                            ctx.dispatch_app_update(move |app| {
                                model.update(app, |model, _| {
                                    model.greeting = "event handled".to_owned();
                                });
                            });
                            TuiDispatchEventResult::StopPropagation
                        } else {
                            TuiDispatchEventResult::PropagateToParent
                        }
                    },
                ),
            )
        }
    }

    #[test]
    fn renders_view_from_shared_model_state() {
        App::test((), |mut app| async move {
            let model = app.add_model(|_| GreetingModel {
                greeting: "hello tui".to_string(),
            });
            let (_, view) = app.add_tui_window(|_| GreetingView { model });

            let mut presenter = TuiPresenter::new();
            let frame = app.read(|ctx| {
                view.read(ctx, |view, ctx| {
                    presenter.render_view(view, ctx, TuiSize::new(12, 3))
                })
            });

            assert_eq!(
                frame.buffer.lines(),
                vec![
                    "┌──────────┐".to_string(),
                    "│hello tui │".to_string(),
                    "└──────────┘".to_string(),
                ]
            );
            assert_eq!(presenter.frame_count(), 1);
        });
    }

    #[test]
    fn dispatches_events_through_rendered_element_tree() {
        App::test((), |mut app| async move {
            let model = app.add_model(|_| GreetingModel {
                greeting: "hello tui".to_string(),
            });
            let (_, view) = app.add_tui_window(|_| EventHandlingView {
                model: model.clone(),
            });

            let mut presenter = TuiPresenter::new();
            app.read(|ctx| {
                view.read(ctx, |view, ctx| {
                    presenter.render_view(view, ctx, TuiSize::new(12, 3));
                })
            });

            let event = Event::KeyDown {
                keystroke: Keystroke {
                    key: "enter".to_owned(),
                    ..Default::default()
                },
                chars: String::new(),
                details: KeyEventDetails::default(),
                is_composing: false,
            };

            let result = presenter.dispatch_event(&event, &mut app);

            assert!(result.handled);
            assert_eq!(
                model.read(&app, |model, _| model.greeting.clone()),
                "event handled"
            );
        });
    }
}
