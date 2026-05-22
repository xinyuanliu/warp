use crossterm::event::{
    Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use warpui_core::event::{KeyEventDetails, ModifiersState};
use warpui_core::geometry::vector::{vec2f, Vector2F};
use warpui_core::keymap::Keystroke;
use warpui_core::{App, Event};

pub enum TuiDispatchEventResult {
    PropagateToParent,
    StopPropagation,
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct TuiEventDispatchResult {
    pub handled: bool,
}

#[derive(Default)]
pub struct TuiEventContext {
    updates: Vec<Box<dyn FnOnce(&mut App)>>,
}

impl TuiEventContext {
    pub fn dispatch_app_update<F>(&mut self, update: F)
    where
        F: 'static + FnOnce(&mut App),
    {
        self.updates.push(Box::new(update));
    }

    pub(crate) fn take_updates(&mut self) -> Vec<Box<dyn FnOnce(&mut App)>> {
        std::mem::take(&mut self.updates)
    }
}

pub fn crossterm_event_to_warp_event(event: CrosstermEvent) -> Option<Event> {
    match event {
        CrosstermEvent::Key(event) => key_event_to_warp_event(event),
        CrosstermEvent::Mouse(event) => mouse_event_to_warp_event(event),
        CrosstermEvent::FocusGained
        | CrosstermEvent::FocusLost
        | CrosstermEvent::Paste(_)
        | CrosstermEvent::Resize(_, _) => None,
    }
}

fn key_event_to_warp_event(event: KeyEvent) -> Option<Event> {
    if event.kind != KeyEventKind::Press {
        return None;
    }

    let key = key_name(event.code, event.modifiers)?;
    let chars = match event.code {
        CrosstermKeyCode::Char(char) => char.to_string(),
        _ => String::new(),
    };

    Some(Event::KeyDown {
        keystroke: Keystroke {
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            shift: event.modifiers.contains(KeyModifiers::SHIFT),
            cmd: event.modifiers.contains(KeyModifiers::SUPER),
            meta: event.modifiers.contains(KeyModifiers::META),
            key,
        },
        chars,
        details: KeyEventDetails {
            key_without_modifiers: key_without_modifiers(event.code),
            ..Default::default()
        },
        is_composing: false,
    })
}

fn key_name(code: CrosstermKeyCode, modifiers: KeyModifiers) -> Option<String> {
    match code {
        CrosstermKeyCode::Backspace => Some("backspace".to_owned()),
        CrosstermKeyCode::Enter => Some("enter".to_owned()),
        CrosstermKeyCode::Left => Some("left".to_owned()),
        CrosstermKeyCode::Right => Some("right".to_owned()),
        CrosstermKeyCode::Up => Some("up".to_owned()),
        CrosstermKeyCode::Down => Some("down".to_owned()),
        CrosstermKeyCode::Home => Some("home".to_owned()),
        CrosstermKeyCode::End => Some("end".to_owned()),
        CrosstermKeyCode::PageUp => Some("pageup".to_owned()),
        CrosstermKeyCode::PageDown => Some("pagedown".to_owned()),
        CrosstermKeyCode::Tab => Some("\t".to_owned()),
        CrosstermKeyCode::BackTab => Some("\t".to_owned()),
        CrosstermKeyCode::Delete => Some("delete".to_owned()),
        CrosstermKeyCode::Insert => Some("insert".to_owned()),
        CrosstermKeyCode::F(key) if key <= 20 => Some(format!("f{key}")),
        CrosstermKeyCode::Char(' ') => Some(" ".to_owned()),
        CrosstermKeyCode::Char(char) if modifiers.contains(KeyModifiers::SHIFT) => {
            Some(char.to_string())
        }
        CrosstermKeyCode::Char(char) => Some(char.to_lowercase().to_string()),
        CrosstermKeyCode::Esc => Some("escape".to_owned()),
        CrosstermKeyCode::Null
        | CrosstermKeyCode::CapsLock
        | CrosstermKeyCode::ScrollLock
        | CrosstermKeyCode::NumLock
        | CrosstermKeyCode::PrintScreen
        | CrosstermKeyCode::Pause
        | CrosstermKeyCode::Menu
        | CrosstermKeyCode::KeypadBegin
        | CrosstermKeyCode::Media(_)
        | CrosstermKeyCode::Modifier(_)
        | CrosstermKeyCode::F(_) => None,
    }
}

fn key_without_modifiers(code: CrosstermKeyCode) -> Option<String> {
    match code {
        CrosstermKeyCode::Char(char) => Some(char.to_lowercase().to_string()),
        _ => None,
    }
}

fn mouse_event_to_warp_event(event: MouseEvent) -> Option<Event> {
    let position = mouse_position(event.column, event.row);
    let modifiers = modifiers_state(event.modifiers);
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => Some(Event::LeftMouseDown {
            position,
            modifiers,
            click_count: 1,
            is_first_mouse: false,
        }),
        MouseEventKind::Up(MouseButton::Left) => Some(Event::LeftMouseUp {
            position,
            modifiers,
        }),
        MouseEventKind::Drag(MouseButton::Left) => Some(Event::LeftMouseDragged {
            position,
            modifiers,
        }),
        MouseEventKind::Down(MouseButton::Middle) => Some(Event::MiddleMouseDown {
            position,
            cmd: modifiers.cmd,
            shift: modifiers.shift,
            click_count: 1,
        }),
        MouseEventKind::Down(MouseButton::Right) => Some(Event::RightMouseDown {
            position,
            cmd: modifiers.cmd,
            shift: modifiers.shift,
            click_count: 1,
        }),
        MouseEventKind::Moved => Some(Event::MouseMoved {
            position,
            cmd: modifiers.cmd,
            shift: modifiers.shift,
            is_synthetic: false,
        }),
        MouseEventKind::ScrollUp => Some(scroll_wheel_event(position, modifiers, vec2f(0.0, 1.0))),
        MouseEventKind::ScrollDown => {
            Some(scroll_wheel_event(position, modifiers, vec2f(0.0, -1.0)))
        }
        MouseEventKind::ScrollLeft => {
            Some(scroll_wheel_event(position, modifiers, vec2f(-1.0, 0.0)))
        }
        MouseEventKind::ScrollRight => {
            Some(scroll_wheel_event(position, modifiers, vec2f(1.0, 0.0)))
        }
        MouseEventKind::Up(MouseButton::Middle | MouseButton::Right)
        | MouseEventKind::Drag(MouseButton::Middle | MouseButton::Right) => None,
    }
}

fn mouse_position(column: u16, row: u16) -> Vector2F {
    vec2f(f32::from(column), f32::from(row))
}

fn scroll_wheel_event(position: Vector2F, modifiers: ModifiersState, delta: Vector2F) -> Event {
    Event::ScrollWheel {
        position,
        delta,
        precise: false,
        modifiers,
    }
}

fn modifiers_state(modifiers: KeyModifiers) -> ModifiersState {
    ModifiersState {
        alt: modifiers.contains(KeyModifiers::ALT),
        cmd: modifiers.contains(KeyModifiers::SUPER),
        shift: modifiers.contains(KeyModifiers::SHIFT),
        ctrl: modifiers.contains(KeyModifiers::CONTROL),
        func: false,
    }
}
