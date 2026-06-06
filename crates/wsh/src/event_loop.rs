use std::io::{self, Read};
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::{Context, Result};
use crossterm::terminal;
use nix::unistd::{close, pipe, read, write as nix_write};

use crate::block::BlockManager;
use crate::cell::{CellFlags, Color};
use crate::miniterm::MiniTerm;
use crate::pty::resize_pty;
use crate::screen::{self, Frame, StatusBar};
use crate::shell_integration::{OscParser, ShellEvent};

// ── SIGWINCH self-pipe ───────────────────────────────────────────────

static SIGWINCH_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn sigwinch_handler(_sig: libc::c_int) {
    let fd = SIGWINCH_WRITE_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        unsafe {
            libc::write(fd, b"W".as_ptr() as *const libc::c_void, 1);
        }
    }
}

fn install_sigwinch_handler() -> Result<RawFd> {
    let (read_fd, write_fd) = pipe().context("pipe for SIGWINCH")?;
    for fd in [read_fd, write_fd] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    }
    SIGWINCH_WRITE_FD.store(write_fd, Ordering::Relaxed);
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigwinch_handler as usize;
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut()) < 0 {
            return Err(anyhow::anyhow!(
                "sigaction SIGWINCH: {}",
                io::Error::last_os_error()
            ));
        }
    }
    Ok(read_fd)
}

// ── Mode state machine ──────────────────────────────────────────────

enum Mode {
    Shell,
    AgentInput { buffer: String, cursor: usize },
    AgentRunning,
}

impl Mode {
    fn label(&self) -> &'static str {
        match self {
            Mode::Shell => "SHELL",
            Mode::AgentInput { .. } => "AGENT",
            Mode::AgentRunning => "RUNNING",
        }
    }

    fn hint(&self) -> &'static str {
        match self {
            Mode::Shell => "Ctrl-A: agent",
            Mode::AgentInput { .. } => "Enter: run | Esc: cancel",
            Mode::AgentRunning => "Ctrl-C: cancel",
        }
    }
}

// ── Interleaved OSC parser ──────────────────────────────────────────

enum ParsedItem {
    Output(Vec<u8>),
    Event(ShellEvent),
}

fn feed_interleaved(parser: &mut OscParser, input: &[u8]) -> Vec<ParsedItem> {
    let mut items = Vec::new();
    for &byte in input {
        let (out, events) = parser.feed(&[byte]);
        if !out.is_empty() {
            if let Some(ParsedItem::Output(ref mut prev)) = items.last_mut() {
                prev.extend_from_slice(&out);
            } else {
                items.push(ParsedItem::Output(out));
            }
        }
        for event in events {
            items.push(ParsedItem::Event(event));
        }
    }
    items
}

// ── Public entry point ──────────────────────────────────────────────

pub fn run(master_fd: RawFd) -> Result<()> {
    let sigwinch_fd = install_sigwinch_handler()?;

    let (cols, rows) = terminal::size().context("terminal::size")?;
    let usable = rows.saturating_sub(1);
    resize_pty(master_fd, cols, usable)?;

    screen::enter_alt_screen().context("enter_alt_screen")?;

    let mut state = Wsh {
        mode: Mode::Shell,
        master_fd,
        osc_parser: OscParser::new(),
        miniterm: MiniTerm::new(cols, usable),
        blocks: BlockManager::new(),
        cols,
        rows,
        should_exit: false,
    };
    state.render_frame();

    let result = state.run_loop(sigwinch_fd);

    let _ = screen::leave_alt_screen();

    let wfd = SIGWINCH_WRITE_FD.swap(-1, Ordering::Relaxed);
    if wfd >= 0 {
        let _ = close(wfd);
    }
    let _ = close(sigwinch_fd);

    result
}

// ── Core state ──────────────────────────────────────────────────────

struct Wsh {
    mode: Mode,
    master_fd: RawFd,
    osc_parser: OscParser,
    miniterm: MiniTerm,
    blocks: BlockManager,
    cols: u16,
    rows: u16,
    should_exit: bool,
}

impl Wsh {
    fn run_loop(&mut self, sigwinch_fd: RawFd) -> Result<()> {
        let stdin_fd: RawFd = libc::STDIN_FILENO;
        let mut buf = [0u8; 4096];

        loop {
            if self.should_exit {
                break;
            }

            let mut pollfds = [
                libc::pollfd { fd: stdin_fd, events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.master_fd, events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: sigwinch_fd, events: libc::POLLIN, revents: 0 },
            ];

            let ret = unsafe {
                libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, -1)
            };
            if ret < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err).context("poll");
            }

            let mut needs_render = false;

            if pollfds[2].revents & libc::POLLIN != 0 {
                self.handle_resize(sigwinch_fd)?;
                needs_render = true;
            }

            if pollfds[1].revents & libc::POLLIN != 0 {
                match read(self.master_fd, &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        self.handle_pty_output(&buf[..n]);
                        needs_render = true;
                    }
                }
            }

            if pollfds[1].revents & libc::POLLHUP != 0 {
                self.drain_pty(&mut buf);
                self.render_frame();
                break;
            }

            if pollfds[0].revents & libc::POLLIN != 0 {
                let n = io::stdin().read(&mut buf).context("read stdin")?;
                if n == 0 {
                    break;
                }
                self.handle_stdin(&buf[..n])?;
                needs_render = true;
            }

            if needs_render {
                self.render_frame();
            }
        }

        Ok(())
    }

    // ── Render ────────────────────────────────────────────────────────

    fn render_frame(&self) {
        let completed = self.blocks.collected_rows();
        let frame = Frame {
            completed_rows: &completed,
            active_grid: self.miniterm.grid(),
            active_cursor: self.miniterm.cursor_pos(),
            status_bar: StatusBar {
                mode: self.mode.label(),
                model: "mock",
                hint: self.mode.hint(),
            },
            scroll_offset: self.blocks.scroll_offset(),
            agent_input: match &self.mode {
                Mode::AgentInput { buffer, cursor } => Some((buffer.as_str(), *cursor)),
                _ => None,
            },
            total_rows: self.rows,
            total_cols: self.cols,
            show_cursor: matches!(self.mode, Mode::Shell),
        };
        let _ = screen::render(&frame);
    }

    // ── Resize ────────────────────────────────────────────────────────

    fn handle_resize(&mut self, sigwinch_fd: RawFd) -> Result<()> {
        let mut drain = [0u8; 64];
        while let Ok(n) = read(sigwinch_fd, &mut drain) {
            if n == 0 { break; }
        }
        let (cols, rows) = terminal::size().context("terminal::size on resize")?;
        self.cols = cols;
        self.rows = rows;
        let usable = rows.saturating_sub(1);
        resize_pty(self.master_fd, cols, usable)?;
        self.miniterm.resize(cols, usable);
        Ok(())
    }

    // ── PTY output ────────────────────────────────────────────────────

    fn handle_pty_output(&mut self, raw: &[u8]) {
        let items = feed_interleaved(&mut self.osc_parser, raw);
        for item in items {
            match item {
                ParsedItem::Output(bytes) => self.miniterm.process_bytes(&bytes),
                ParsedItem::Event(event) => self.process_event(event),
            }
        }
    }

    fn process_event(&mut self, event: ShellEvent) {
        match event {
            ShellEvent::PromptStart => {
                self.capture_completed_block();
                self.blocks.scroll_to_bottom();
            }
            ShellEvent::PromptEnd => {
                if matches!(self.mode, Mode::AgentRunning) {
                    self.mode = Mode::Shell;
                }
            }
            _ => {}
        }
    }

    fn capture_completed_block(&mut self) {
        let mut rows = self.miniterm.take_scrolled_out();
        rows.extend(self.miniterm.grid().iter().cloned());
        self.blocks.add_block(rows);
        let usable = self.rows.saturating_sub(1);
        self.miniterm.resize(self.cols, usable);
    }

    fn drain_pty(&mut self, buf: &mut [u8]) {
        loop {
            match read(self.master_fd, buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let (filtered, _) = self.osc_parser.feed(&buf[..n]);
                    if !filtered.is_empty() {
                        self.miniterm.process_bytes(&filtered);
                    }
                }
            }
        }
    }

    // ── Stdin handling ────────────────────────────────────────────────

    fn handle_stdin(&mut self, bytes: &[u8]) -> Result<()> {
        match &self.mode {
            Mode::Shell => {
                if let Some(pos) = bytes.iter().position(|&b| b == 0x01) {
                    if pos > 0 {
                        nix_write(self.master_fd, &bytes[..pos]).context("write to pty")?;
                    }
                    self.enter_agent_mode();
                    if pos + 1 < bytes.len() {
                        return self.handle_agent_input_bytes(&bytes[pos + 1..]);
                    }
                } else {
                    self.blocks.scroll_to_bottom();
                    nix_write(self.master_fd, bytes).context("write to pty")?;
                }
            }
            Mode::AgentInput { .. } => {
                self.handle_agent_input_bytes(bytes)?;
            }
            Mode::AgentRunning => {
                if bytes.contains(&0x03) {
                    self.cancel_agent();
                }
            }
        }
        Ok(())
    }

    // ── Agent input mode ──────────────────────────────────────────────

    fn enter_agent_mode(&mut self) {
        self.mode = Mode::AgentInput {
            buffer: String::new(),
            cursor: 0,
        };
    }

    fn exit_agent_mode(&mut self) {
        self.mode = Mode::Shell;
        let _ = nix_write(self.master_fd, b"\n");
    }

    fn cancel_agent(&mut self) {
        self.mode = Mode::Shell;
        let _ = nix_write(self.master_fd, b"\x03");
    }

    fn handle_agent_input_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        for &b in bytes {
            match b {
                0x0d | 0x0a => return self.submit_agent_query(),
                0x1b | 0x03 => {
                    self.exit_agent_mode();
                    return Ok(());
                }
                0x7f | 0x08 => {
                    if let Mode::AgentInput { buffer, cursor } = &mut self.mode {
                        if *cursor > 0 {
                            *cursor -= 1;
                            buffer.remove(*cursor);
                        }
                    }
                }
                b if (0x20..0x7f).contains(&b) => {
                    if let Mode::AgentInput { buffer, cursor } = &mut self.mode {
                        buffer.insert(*cursor, b as char);
                        *cursor += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // ── Mock agent ────────────────────────────────────────────────────

    fn submit_agent_query(&mut self) -> Result<()> {
        let command = match &self.mode {
            Mode::AgentInput { buffer, .. } => buffer.clone(),
            _ => return Ok(()),
        };

        if command.trim().is_empty() {
            self.exit_agent_mode();
            return Ok(());
        }

        match command.trim() {
            "help" => {
                self.blocks.add_styled_line(
                    "🤖 Type any command. Special: help, exit",
                    Color::Indexed(6),
                    CellFlags::DIM,
                    self.cols as usize,
                );
                self.mode = Mode::Shell;
                let _ = nix_write(self.master_fd, b"\n");
                return Ok(());
            }
            "exit" => {
                self.should_exit = true;
                return Ok(());
            }
            _ => {}
        }

        self.blocks.add_styled_line(
            &format!("🤖 running: {command}"),
            Color::Indexed(5),
            CellFlags::DIM,
            self.cols as usize,
        );

        let cmd = format!("{command}\n");
        nix_write(self.master_fd, cmd.as_bytes()).context("write command to pty")?;

        self.mode = Mode::AgentRunning;
        Ok(())
    }
}
