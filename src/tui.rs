use crate::{
    model::{Attempt, Language, Mode, Profile, Rep},
    process::{self, Cancel},
    runner::{self, ContainerEngine, Evaluation},
    service,
    store::Store,
};
use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
mod brief;
mod editor;
mod syntax;
use editor::CodeEditor;
use std::{
    io,
    process::Command,
    sync::{
        atomic::Ordering,
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const BG: Color = Color::Rgb(0, 0, 0);
const PANEL: Color = BG;
const INK: Color = Color::Rgb(224, 224, 224);
const MUTED: Color = Color::Rgb(160, 160, 160);
const ACCENT: Color = Color::Rgb(255, 255, 255);
const EDGE: Color = Color::Rgb(96, 96, 96);
const SUCCESS: Color = Color::Rgb(224, 224, 224);
const WARNING: Color = Color::Rgb(240, 240, 240);
const ERROR: Color = Color::Rgb(255, 255, 255);
const INFO: Color = Color::Rgb(208, 208, 208);

enum Message {
    Status(String),
    Prepared(Result<usize>),
    Assessed(Result<Evaluation>),
}
struct Job {
    cancel: Cancel,
    rx: Receiver<Message>,
    handle: JoinHandle<()>,
}
#[derive(PartialEq)]
enum Overlay {
    Solution,
    Replace,
    Help,
    Find,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Brief,
    Code,
    Tests,
    Results,
    Output,
}
impl Pane {
    fn next(self) -> Self {
        match self {
            Self::Brief => Self::Code,
            Self::Code => Self::Tests,
            Self::Tests => Self::Results,
            Self::Results => Self::Output,
            Self::Output => Self::Brief,
        }
    }
}

#[derive(Default)]
struct Workspace {
    brief: Rect,
    code: Rect,
    tests: Rect,
    results: Rect,
    output: Rect,
}

struct App {
    profile: Profile,
    rep: Option<Rep>,
    attempt: Option<Attempt>,
    code: CodeEditor,
    tests: CodeEditor,
    focus: Pane,
    brief_scroll: u16,
    brief_cache: brief::Cache,
    results_scroll: u16,
    output_scroll: u16,
    results_stale: bool,
    showing_history: bool,
    workspace: Workspace,
    status: String,
    result: Option<Evaluation>,
    job: Option<Job>,
    overlay: Option<Overlay>,
    show_solution: bool,
    demo: bool,
    history: String,
    active: Duration,
    last_input: Instant,
    tick: Instant,
    save_at: Instant,
    search: String,
    search_pane: Pane,
    started: Instant,
    status_error: bool,
    clipboard: Option<arboard::Clipboard>,
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(EDGE))
        .style(Style::default().bg(PANEL).fg(INK))
}

fn shortcut_line(actions: &[(&str, &str)]) -> Line<'static> {
    Line::from(
        actions
            .iter()
            .flat_map(|(key, label)| {
                [
                    Span::styled(format!(" {key} "), Style::default().fg(INK).bg(EDGE).bold()),
                    Span::styled(format!("{label}  "), Style::default().fg(MUTED)),
                ]
            })
            .collect::<Vec<_>>(),
    )
}

fn help_text() -> Text<'static> {
    let sections = [
        (
            "MOVE AROUND",
            "Alt+1–4 / Shift+Tab   Focus a pane\nAlt+↑/↓ / Alt+PgUp/Dn Scroll the brief while editing\nClick / drag / wheel Place cursor, select, scroll\nAlt+O               Expand debug output",
        ),
        (
            "EDIT",
            "Ctrl+Z / Ctrl+Y       Undo / redo\nCtrl+C / Ctrl+X / Ctrl+V  Copy / cut / paste\nShift+arrows / Ctrl+A Select text / select all\nTab / Enter          Indent / newline with indentation\nCtrl+F / Ctrl+G       Find / next match\nCtrl+S / F3           Save / external editor",
        ),
        (
            "PRACTISE",
            "F1 brief   F2 hint   F4 solution   F5 run checks\nF6 next    F7 history   F8 replace   F9 prepare more\nEsc save / cancel    Ctrl+Q save and quit",
        ),
    ];
    let mut lines = Vec::new();
    for (heading, body) in sections {
        lines.push(Line::styled(heading, Style::default().fg(ACCENT).bold()));
        lines.extend(body.lines().map(Line::raw));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Any key returns to your rep.",
        Style::default().fg(MUTED),
    ));
    Text::from(lines)
}

/// Logs are text, never terminal escape sequences or control instructions.
fn clean_output(source: &str) -> String {
    let mut result = String::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\n' => result.push('\n'),
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    result.push('\n');
                }
            }
            '\t' => result.push_str("    "),
            c if !c.is_control() => result.push(c),
            _ => {}
        }
    }
    result
}

/// Keep document scroll bounded and show how much remains, including after resize.
fn draw_document(
    frame: &mut Frame,
    text: Text<'static>,
    area: Rect,
    block: Block<'static>,
    scroll: &mut u16,
) {
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    let height = area.height.saturating_sub(2) as usize;
    let total = paragraph.line_count(area.width.saturating_sub(2));
    let max_scroll = total.saturating_sub(height).min(u16::MAX as usize) as u16;
    *scroll = (*scroll).min(max_scroll);
    let block = if max_scroll > 0 {
        block.title_bottom(
            Line::styled(
                format!(
                    " {}–{} / {} ↕ ",
                    *scroll as usize + 1,
                    (*scroll as usize + height).min(total),
                    total
                ),
                Style::default().fg(MUTED),
            )
            .right_aligned(),
        )
    } else {
        block
    };
    frame.render_widget(paragraph.block(block).scroll((*scroll, 0)), area);
}

impl App {
    fn new(profile: Profile, demo: bool) -> Self {
        let language = profile.language;
        Self {
            profile,
            rep: None,
            attempt: None,
            code: CodeEditor::new("", language),
            tests: CodeEditor::new("", language),
            focus: Pane::Code,
            brief_scroll: 0,
            brief_cache: brief::Cache::default(),
            results_scroll: 0,
            output_scroll: 0,
            results_stale: false,
            showing_history: false,
            workspace: Workspace::default(),
            status: "Ready for a little practice.".into(),
            result: None,
            job: None,
            overlay: None,
            show_solution: false,
            demo,
            history: String::new(),
            active: Duration::ZERO,
            last_input: Instant::now(),
            tick: Instant::now(),
            save_at: Instant::now(),
            search: String::new(),
            search_pane: Pane::Code,
            started: Instant::now(),
            status_error: false,
            clipboard: None,
        }
    }
    fn load(&mut self, rep: Rep, attempt: Attempt) {
        self.code = CodeEditor::new(&attempt.code, rep.language);
        self.tests = CodeEditor::new(&attempt.tests, rep.language);
        self.tests.set_placeholder(if rep.mode == Mode::Build {
            "Optional: write your own assertions here. Examples are in Brief."
        } else {
            "Write your own assertions here. Examples are in Brief."
        });
        self.status_error = false;
        self.active = Duration::from_secs(attempt.active_seconds);
        self.focus = if rep.mode == Mode::Test {
            Pane::Tests
        } else {
            Pane::Code
        };
        self.rep = Some(rep);
        self.attempt = Some(attempt);
        self.result = None;
        self.results_stale = false;
        self.output_scroll = 0;
        self.brief_scroll = 0;
        self.results_scroll = 0;
        self.showing_history = false;
        self.show_solution = false;
        self.status = if self.demo {
            "Preview · temporary workspace · no AI calls"
        } else {
            "Edit code and tests · F5 checks your work"
        }
        .into();
    }
    fn save(&mut self, store: &mut Store) -> Result<()> {
        if let Some(a) = &mut self.attempt {
            a.code = self.code.lines().join("\n") + "\n";
            a.tests = self.tests.lines().join("\n") + "\n";
            a.active_seconds = self.active.as_secs();
            store.save_attempt(a)?;
            self.code.mark_saved();
            self.tests.mark_saved();
        }
        self.save_at = Instant::now();
        Ok(())
    }
    fn done(&self) -> bool {
        self.attempt
            .as_ref()
            .is_some_and(|a| a.outcome != "in-progress")
    }
    fn next(&mut self, store: &mut Store) -> Result<()> {
        if let Some((rep, attempt)) = service::next_cached(store, &self.profile)? {
            self.load(rep, attempt);
        } else if self.demo {
            self.status = "Preview complete. Run spar init to start your own practice.".into();
        } else {
            self.rep = None;
            self.attempt = None;
            self.code = CodeEditor::new("", self.profile.language);
            self.tests = CodeEditor::new("", self.profile.language);
            self.code
                .set_placeholder("Your next rep's code will appear here.");
            self.tests
                .set_placeholder("Tests become editable when a rep is ready.");
            self.result = None;
            self.results_stale = false;
            self.show_solution = false;
            self.showing_history = false;
            self.overlay = None;
            self.active = Duration::ZERO;
            self.brief_scroll = 0;
            self.results_scroll = 0;
            self.output_scroll = 0;
            self.prepare(store);
        }
        Ok(())
    }
    fn prepare(&mut self, store: &Store) {
        if self.job.is_some() || self.demo {
            return;
        }
        let root = store.root.clone();
        self.status_error = false;
        let profile = self.profile.clone();
        let cancel = process::cancel_token();
        let worker_cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.status = "Preparing a rep · Esc cancels".into();
        let handle = thread::spawn(move || {
            let result = Store::open(Some(&root)).and_then(|s| {
                service::prepare(&s, &profile, 1, false, &worker_cancel, |message| {
                    let _ = tx.send(Message::Status(message.into()));
                })
            });
            let _ = tx.send(Message::Prepared(result));
        });
        self.job = Some(Job { cancel, rx, handle });
    }
    fn assess(&mut self, store: &mut Store) -> Result<()> {
        if self.job.is_some() || self.done() {
            return Ok(());
        }
        self.save(store)?;
        self.status_error = false;
        let (Some(rep), Some(a)) = (self.rep.clone(), self.attempt.clone()) else {
            return Ok(());
        };
        let cancel = process::cancel_token();
        let worker_cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.status = "Checking tests · Esc cancels".into();
        let handle = thread::spawn(move || {
            let result = ContainerEngine::configured().and_then(|engine| {
                engine.available(rep.language, &worker_cancel)?;
                runner::assess(&rep, &a.code, &a.tests, &engine, &worker_cancel)
            });
            let _ = tx.send(Message::Assessed(result));
        });
        self.job = Some(Job { cancel, rx, handle });
        Ok(())
    }
    fn poll(&mut self, store: &mut Store) -> Result<()> {
        let mut messages = Vec::new();
        let mut disconnected = false;
        if let Some(job) = &self.job {
            loop {
                match job.rx.try_recv() {
                    Ok(message) => messages.push(message),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for message in messages {
            match message {
                Message::Status(s) => self.status = s,
                Message::Prepared(result) => {
                    if let Some(job) = self.job.take() {
                        let _ = job.handle.join();
                    }
                    match result {
                        Ok(n) => {
                            if self.rep.is_none() {
                                self.next(store)?;
                            } else {
                                self.status = format!("{n} more rep ready for later.");
                            }
                        }
                        Err(e) => self.error(format!("{e:#}")),
                    }
                }
                Message::Assessed(result) => {
                    if let Some(job) = self.job.take() {
                        let _ = job.handle.join();
                    }
                    match result {
                        Ok(result) => {
                            if result.passed() {
                                if let Some(a) = &mut self.attempt {
                                    a.outcome = "passed".into();
                                }
                                self.status="Rep complete. A little progress, recorded. F6 for your next rep.".into();
                                self.save(store)?;
                            } else {
                                self.status =
                                    "Checks finished · Alt+4 results · Alt+O output".into();
                            }
                            self.result = Some(result);
                            self.results_stale = false;
                            self.output_scroll = 0;
                            self.results_scroll = 0;
                            self.showing_history = false;
                        }
                        Err(e) => self.error(format!("{e:#}")),
                    }
                }
            }
        }
        if disconnected && self.job.as_ref().is_some_and(|j| j.handle.is_finished()) {
            // A worker panic or disconnected channel must not leave the UI stuck.
            if let Some(job) = self.job.take()
                && job.handle.join().is_err()
            {
                self.status =
                    "Background operation stopped unexpectedly. Your work is saved.".into();
            }
        }
        Ok(())
    }
    fn hint(&mut self, store: &mut Store) -> Result<()> {
        if let (Some(rep), Some(a)) = (&self.rep, &mut self.attempt) {
            if a.outcome == "in-progress" {
                a.hints = (a.hints + 1).min(rep.hints.len());
            }
            self.brief_scroll = 0;
            self.status = format!("Hint {} of {} revealed", a.hints, rep.hints.len());
        }
        self.save(store)
    }
    fn history(&mut self, store: &Store) -> Result<()> {
        let mut lines = vec![format!("{} / observed practice\n", self.profile.name)];
        for a in store.attempts(&self.profile.name)? {
            let rep = store.rep(&a.rep_id)?;
            lines.push(format!(
                "{}\n{} · {} · {} · {}m {}s\n",
                rep.title,
                rep.skill,
                a.outcome,
                a.assistance(),
                a.active_seconds / 60,
                a.active_seconds % 60
            ));
        }
        if lines.len() == 1 {
            lines.push("Your first rep starts here.".into());
        }
        self.history = lines.join("\n");
        self.focus = Pane::Results;
        self.showing_history = true;
        self.results_scroll = 0;
        Ok(())
    }
    fn external(
        &mut self,
        store: &mut Store,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<()> {
        if self.job.is_some() || self.done() {
            return Ok(());
        }
        self.save(store)?;
        let (Some(a), Some(rep)) = (&self.attempt, &self.rep) else {
            return Ok(());
        };
        let dir = store.root.join("workspaces").join(&a.id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(rep.language.file()), &a.code)?;
        std::fs::write(dir.join(rep.language.test_file()), &a.tests)?;
        let tests = self.focus == Pane::Tests || rep.mode == Mode::Test;
        let file = dir.join(if tests {
            rep.language.test_file()
        } else {
            rep.language.file()
        });
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".into());
        let args = shell_words::split(&editor).context("Invalid VISUAL/EDITOR command")?;
        let executable = args.first().context("EDITOR cannot be empty")?;
        execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture)?;
        ratatui::restore();
        let result = Command::new(executable)
            .args(&args[1..])
            .arg(&file)
            .status();
        *terminal = ratatui::try_init()?;
        execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
        self.tick = Instant::now();
        self.last_input = Instant::now();
        let status = result.context("Could not open external editor")?;
        anyhow::ensure!(status.success(), "External editor exited unsuccessfully");
        anyhow::ensure!(
            std::fs::metadata(&file)?.len() <= 32_000,
            "Edited file exceeds 32 KB"
        );
        let text = std::fs::read_to_string(file)?;
        if tests {
            self.tests = CodeEditor::new(&text, rep.language);
        } else {
            self.code = CodeEditor::new(&text, rep.language);
        }
        self.save(store)?;
        self.results_stale = self.result.is_some();
        self.status = "External changes loaded and saved.".into();
        Ok(())
    }
    fn error(&mut self, message: String) {
        self.status = message;
        self.status_error = true;
        self.results_scroll = 0;
        self.showing_history = false;
    }
    fn key(
        &mut self,
        key: KeyEvent,
        store: &mut Store,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<bool> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('q') {
            self.save(store)?;
            if let Some(job) = self.job.take() {
                job.cancel.store(true, Ordering::Relaxed);
                let _ = job.handle.join();
            }
            return Ok(true);
        }
        if let Some(overlay) = self.overlay.take() {
            if overlay == Overlay::Find {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        self.find_next();
                        self.overlay = Some(Overlay::Find);
                    }
                    KeyCode::Backspace => {
                        self.search.pop();
                        self.overlay = Some(Overlay::Find);
                    }
                    KeyCode::Char(c) if !control && self.search.len() < 200 => {
                        self.search.push(c);
                        self.overlay = Some(Overlay::Find);
                    }
                    _ => self.overlay = Some(Overlay::Find),
                }
                return Ok(false);
            }
            match (overlay, key.code) {
                (Overlay::Solution, KeyCode::Enter) => {
                    if let Some(a) = &mut self.attempt
                        && a.outcome == "in-progress"
                    {
                        a.viewed_solution = true;
                    }
                    self.show_solution = true;
                    self.focus = Pane::Results;
                    self.showing_history = false;
                    self.results_scroll = 0;
                    self.save(store)?;
                }
                (Overlay::Replace, KeyCode::Char(c @ '1'..='3')) => {
                    if let Some(a) = &mut self.attempt {
                        a.outcome = match c {
                            '1' => "too-large",
                            '2' => "unclear",
                            _ => "broken",
                        }
                        .into();
                    }
                    self.save(store)?;
                    self.next(store)?;
                }
                _ => {}
            }
            return Ok(false);
        }
        if control && matches!(key.code, KeyCode::Char('c' | 'x' | 'v')) {
            self.clipboard(key.code)?;
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => {
                self.code.clear_selection();
                self.tests.clear_selection();
                if self.showing_history || self.show_solution {
                    self.showing_history = false;
                    self.show_solution = false;
                    self.results_scroll = 0;
                    self.focus = if self.rep.as_ref().is_some_and(|r| r.mode == Mode::Test) {
                        Pane::Tests
                    } else {
                        Pane::Code
                    };
                }
                if let Some(job) = &self.job {
                    job.cancel.store(true, Ordering::Relaxed);
                    self.status = "Cancelling…".into();
                } else {
                    self.save(store)?;
                    self.status = "Saved. Ctrl+Q to close; spar resumes this rep.".into();
                }
            }
            KeyCode::F(1) => {
                self.focus = Pane::Brief;
                self.brief_scroll = 0;
            }
            KeyCode::F(2) => self.hint(store)?,
            KeyCode::F(3) => self.external(store, terminal)?,
            KeyCode::F(4) if self.rep.is_some() => self.overlay = Some(Overlay::Solution),
            KeyCode::F(5) => self.assess(store)?,
            KeyCode::F(6) if self.job.is_none() => {
                if self.done() || self.rep.is_none() {
                    self.next(store)?;
                } else {
                    self.status = "Finish or replace this rep before starting the next.".into();
                }
            }
            KeyCode::F(7) => self.history(store)?,
            KeyCode::F(8) if self.rep.is_some() && !self.done() && self.job.is_none() => {
                self.overlay = Some(Overlay::Replace)
            }
            KeyCode::F(9) => self.prepare(store),
            KeyCode::F(10) => self.overlay = Some(Overlay::Help),
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.focus = if self.focus == Pane::Output {
                    Pane::Results
                } else {
                    Pane::Output
                };
                self.showing_history = false;
            }
            KeyCode::Char('f') if control => {
                self.search_pane = if self.focus == Pane::Tests {
                    Pane::Tests
                } else {
                    Pane::Code
                };
                self.focus = self.search_pane;
                self.search.clear();
                self.overlay = Some(Overlay::Find);
            }
            KeyCode::Char('g') if control => self.find_next(),
            KeyCode::Char('s') if control => {
                self.save(store)?;
                self.status = "Saved.".into();
            }
            KeyCode::BackTab => self.focus = self.focus.next(),
            KeyCode::Char(c @ '1'..='4') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.focus = [Pane::Brief, Pane::Code, Pane::Tests, Pane::Results]
                    [c as usize - '1' as usize];
                if self.focus == Pane::Results {
                    self.showing_history = false;
                }
            }
            KeyCode::Char('5') if key.modifiers.contains(KeyModifiers::ALT) => {
                self.history(store)?
            }
            _ => {
                self.workspace_key(key);
            }
        }
        Ok(false)
    }
    fn find_next(&mut self) {
        let editor = if self.search_pane == Pane::Tests {
            &mut self.tests
        } else {
            &mut self.code
        };
        self.status = if self.search.is_empty() {
            "Type text to find, then press Enter.".into()
        } else if editor.find(&self.search) {
            "Match selected · Enter / Ctrl+G next · Esc returns to editing".into()
        } else {
            format!("No matches for ‘{}’", self.search)
        };
    }
    fn editable(&self, pane: Pane) -> bool {
        self.rep.is_some()
            && !self.done()
            && self.job.is_none()
            && (pane == Pane::Tests
                || (pane == Pane::Code && !self.rep.as_ref().is_some_and(|r| r.mode == Mode::Test)))
    }
    fn clipboard(&mut self, key: KeyCode) -> Result<()> {
        if !matches!(self.focus, Pane::Code | Pane::Tests) {
            return Ok(());
        }
        if key != KeyCode::Char('c') && !self.editable(self.focus) {
            self.status = "This file is read-only right now.".into();
            return Ok(());
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(
                arboard::Clipboard::new()
                    .context("Clipboard unavailable; use your terminal’s copy/paste shortcuts")?,
            );
        }
        let clipboard = self.clipboard.as_mut().context("Clipboard unavailable")?;
        let editor = if self.focus == Pane::Tests {
            &mut self.tests
        } else {
            &mut self.code
        };
        if key == KeyCode::Char('v') {
            let text = clipboard.get_text().context("Clipboard has no text")?;
            self.paste(text);
        } else if let Some(text) = editor.selected_text() {
            clipboard.set_text(text)?;
            if key == KeyCode::Char('x') {
                if editor.insert_str("") {
                    self.results_stale = self.result.is_some();
                }
                self.status = "Selection cut to clipboard.".into();
            } else {
                self.status = "Selection copied to clipboard.".into();
            }
        } else {
            self.status = "Select text with Shift+arrows or drag to copy.".into();
        }
        Ok(())
    }
    fn paste(&mut self, text: String) {
        if !self.editable(self.focus) {
            return;
        }
        let editor = if self.focus == Pane::Tests {
            &mut self.tests
        } else {
            &mut self.code
        };
        if text.len() > 32_000 {
            self.status = "Paste exceeds the 32 KB file limit.".into();
        } else if editor.insert_str(text) {
            self.results_stale = self.result.is_some();
            self.status_error = false;
        } else if editor.limit_reached {
            self.status = "Each file is limited to 32 KB. Paste was not applied.".into();
        }
    }
    /// Scroll the brief independently of editing focus and route text only to an editable pane.
    fn workspace_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::ALT) {
            let amount = match key.code {
                KeyCode::Up => -1,
                KeyCode::Down => 1,
                KeyCode::PageUp => -5,
                KeyCode::PageDown => 5,
                _ => 0,
            };
            if amount != 0 {
                self.scroll_pane(Pane::Brief, amount);
                return;
            }
        }
        if self.focus == Pane::Code || self.focus == Pane::Tests {
            let readonly = !self.editable(self.focus);
            if readonly {
                match key.code {
                    KeyCode::Up => self.scroll_editor(-1),
                    KeyCode::Down => self.scroll_editor(1),
                    KeyCode::PageUp => self.scroll_editor(-5),
                    KeyCode::PageDown => self.scroll_editor(5),
                    _ => {}
                }
            } else {
                let changed = if self.focus == Pane::Tests {
                    self.tests.input(key)
                } else {
                    self.code.input(key)
                };
                if changed {
                    self.results_stale = self.result.is_some();
                    self.status_error = false;
                }
                if self.code.limit_reached || self.tests.limit_reached {
                    self.status =
                        "Each file is limited to 32 KB. That edit was not applied.".into();
                }
            }
        } else {
            let amount = match key.code {
                KeyCode::Up => -1,
                KeyCode::Down => 1,
                KeyCode::PageUp => -5,
                KeyCode::PageDown => 5,
                _ => 0,
            };
            self.scroll_pane(self.focus, amount);
        }
    }
    fn scroll_editor(&mut self, amount: i16) {
        if self.focus == Pane::Tests {
            self.tests.scroll((amount, 0));
        } else {
            self.code.scroll((amount, 0));
        }
    }
    fn scroll_pane(&mut self, pane: Pane, amount: i16) {
        let scroll = match pane {
            Pane::Brief => &mut self.brief_scroll,
            Pane::Output => &mut self.output_scroll,
            _ => &mut self.results_scroll,
        };
        *scroll = scroll.saturating_add_signed(amount);
    }
    fn mouse(&mut self, event: MouseEvent) {
        if self.overlay.is_some() {
            return;
        }
        let point = Position::new(event.column, event.row);
        let target = [
            (Pane::Brief, self.workspace.brief),
            (Pane::Code, self.workspace.code),
            (Pane::Tests, self.workspace.tests),
            (Pane::Results, self.workspace.results),
            (Pane::Output, self.workspace.output),
        ]
        .into_iter()
        .find_map(|(pane, rect)| rect.contains(point).then_some(pane));
        let Some(pane) = target else {
            return;
        };
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.focus = pane;
                match pane {
                    Pane::Code => self.code.mouse(event),
                    Pane::Tests => self.tests.mouse(event),
                    _ => {}
                }
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                match (pane, self.focus) {
                    (Pane::Code, Pane::Code) => self.code.mouse(event),
                    (Pane::Tests, Pane::Tests) => self.tests.mouse(event),
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let amount = if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                };
                match pane {
                    Pane::Brief | Pane::Results | Pane::Output => self.scroll_pane(pane, amount),
                    Pane::Code => self.code.mouse(event),
                    Pane::Tests => self.tests.mouse(event),
                }
            }
            _ => {}
        }
    }
    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(BG).fg(INK)),
            area,
        );
        if area.width < 68 || area.height < 22 {
            self.workspace = Workspace::default();
            frame.render_widget(Paragraph::new("Spar needs a little more room: 68 × 22.\nResize the terminal, or Ctrl+Q to save and exit.").wrap(Wrap {trim:false}),area);
            return;
        }
        let outer = area.inner(Margin {
            horizontal: 1,
            vertical: if area.height < 30 { 0 } else { 1 },
        });
        let feedback_height = if matches!(self.focus, Pane::Results | Pane::Output) {
            (outer.height / 3).clamp(4, 14)
        } else if area.height >= 30 {
            5
        } else {
            3
        };
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(11),
            Constraint::Length(feedback_height),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(outer);
        let title = self
            .rep
            .as_ref()
            .map(|r| r.title.to_uppercase())
            .unwrap_or_else(|| "MAKE ROOM FOR YOUR NEXT REP".into());
        let meta = self
            .rep
            .as_ref()
            .map(|r| {
                format!(
                    "{} · {} · ~{} min    {}m {:02}s active",
                    r.language,
                    r.mode.task_label(),
                    r.minutes,
                    self.active.as_secs() / 60,
                    self.active.as_secs() % 60
                )
            })
            .unwrap_or_else(|| format!("{} · {}", self.profile.language, self.profile.name));
        let header_columns = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(if area.width >= 100 { 23 } else { 0 }),
        ])
        .split(rows[0]);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" spar ", Style::default().bg(ACCENT).fg(BG).bold()),
                    Span::styled(format!("  {title}"), Style::default().fg(INK).bold()),
                ]),
                Line::styled(format!(" {meta}"), Style::default().fg(MUTED)),
            ]),
            header_columns[0],
        );
        if area.width >= 100 {
            let assistance = self
                .attempt
                .as_ref()
                .map(|a| a.assistance())
                .unwrap_or("No active rep");
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        if self.demo {
                            "PREVIEW".into()
                        } else {
                            self.profile.name.clone()
                        },
                        Style::default().fg(MUTED),
                    ),
                    Line::styled(
                        assistance,
                        Style::default().fg(if self.done() { SUCCESS } else { INFO }),
                    ),
                ])
                .right_aligned(),
                header_columns[1],
            );
        }
        let (brief, editors) = if area.width >= 100 {
            let columns =
                Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                    .split(rows[1]);
            (columns[0], columns[1])
        } else {
            let stack = Layout::vertical([
                Constraint::Length((rows[1].height / 3).clamp(4, 8)),
                Constraint::Min(7),
            ])
            .split(rows[1]);
            (stack[0], stack[1])
        };
        let code_share = if self.focus == Pane::Tests { 40 } else { 62 };
        let edit_rows = Layout::vertical([
            Constraint::Percentage(code_share),
            Constraint::Percentage(100 - code_share),
        ])
        .split(editors);
        self.workspace = Workspace {
            brief,
            code: edit_rows[0],
            tests: edit_rows[1],
            results: rows[2],
            output: Rect::default(),
        };
        let focus = self.focus;
        let block = |title: String, pane| {
            panel(Line::styled(
                title,
                Style::default()
                    .fg(if focus == pane { ACCENT } else { MUTED })
                    .bold(),
            ))
            .border_style(Style::default().fg(if focus == pane {
                ACCENT
            } else {
                EDGE
            }))
        };
        draw_document(
            frame,
            self.brief_text(brief.width.saturating_sub(2)),
            brief,
            block(" 1 Brief ".into(), Pane::Brief),
            &mut self.brief_scroll,
        );
        let language = self.profile.language;
        let frozen = self.rep.is_none() || self.done() || self.job.is_some();
        let code_readonly = frozen || self.rep.as_ref().is_some_and(|r| r.mode == Mode::Test);
        self.code.render(
            frame,
            edit_rows[0],
            block(format!(" 2 Code · {} ", language.file()), Pane::Code),
            focus == Pane::Code && self.overlay.is_none(),
            code_readonly,
        );
        self.tests.render(
            frame,
            edit_rows[1],
            block(format!(" 3 Tests · {} ", language.test_file()), Pane::Tests),
            focus == Pane::Tests && self.overlay.is_none(),
            frozen,
        );
        let feedback = if self.showing_history {
            Text::raw(self.history.clone())
        } else {
            self.results_text()
        };
        let label = if self.showing_history {
            " 4 History · Esc returns "
        } else {
            " 4 Results · Alt+4 expand "
        };
        let (checks_area, output_area) =
            if area.width >= 100 && !self.showing_history && !self.show_solution {
                let split =
                    Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)])
                        .split(rows[2]);
                (split[0], split[1])
            } else if self.focus == Pane::Output {
                (Rect::default(), rows[2])
            } else {
                (rows[2], Rect::default())
            };
        self.workspace.results = checks_area;
        self.workspace.output = output_area;
        if checks_area.width > 0 {
            draw_document(
                frame,
                feedback,
                checks_area,
                block(label.into(), Pane::Results),
                &mut self.results_scroll,
            );
        }
        if output_area.width > 0 {
            draw_document(
                frame,
                self.output_text(),
                output_area,
                block(" Output · Alt+O expand ".into(), Pane::Output),
                &mut self.output_scroll,
            );
        }
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let prefix = if self.job.is_some() {
            frames[(self.started.elapsed().as_millis() / 100) as usize % frames.len()]
        } else if self.status_error {
            "!"
        } else {
            "·"
        };
        let status_columns =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(16)]).split(rows[3]);
        frame.render_widget(
            Paragraph::new(format!("{prefix} {}", self.status))
                .style(Style::default().fg(if self.status_error { ERROR } else { MUTED })),
            status_columns[0],
        );
        let modified = self.code.modified() || self.tests.modified();
        frame.render_widget(
            Paragraph::new(if self.attempt.is_none() {
                "No active rep"
            } else if modified {
                "● Unsaved edits"
            } else {
                "✓ Saved locally"
            })
            .right_aligned()
            .style(Style::default().fg(if modified { WARNING } else { SUCCESS })),
            status_columns[1],
        );
        let mut actions = vec![
            ("F5", "run"),
            ("F2", "hint"),
            ("Ctrl+F", "find"),
            ("F10", "help"),
        ];
        if area.width >= 100 {
            actions.extend([("F3", "editor"), ("F6", "next")]);
        }
        actions.push(("Ctrl+Q", "quit"));
        frame.render_widget(
            Paragraph::new(vec![
                shortcut_line(&actions),
                Line::styled(
                    "Alt+1–4 focus   Alt+O output   Alt+↑/↓ scroll brief",
                    Style::default().fg(MUTED),
                ),
            ]),
            rows[4],
        );
        if let Some(overlay) = &self.overlay {
            let (title, text, height) = match overlay {
                Overlay::Solution => (
                    " Reveal solution ",
                    Text::raw(
                        "Reveal the reference implementation and tests?\n\nThis attempt will record Viewed solution.\n\nEnter reveals it · Any other key returns to your rep",
                    ),
                    9,
                ),
                Overlay::Replace => (
                    " Replace this rep ",
                    Text::raw(
                        "What got in the way?\n\n1  Too large     2  Unclear     3  Ambiguous / broken\n\nThe reason stays local. This is not a skill failure.\nAny other key returns to your rep.",
                    ),
                    10,
                ),
                Overlay::Help => (" Spar · Keyboard guide ", help_text(), 20),
                Overlay::Find => (
                    " Find in current file ",
                    Text::from(vec![
                        Line::styled(format!("{}▏", self.search), Style::default().fg(INK)),
                        Line::raw(""),
                        Line::styled(
                            "Enter next match · Esc close · Ctrl+G repeat later",
                            Style::default().fg(MUTED),
                        ),
                    ]),
                    5,
                ),
            };
            let width = 62.min(area.width.saturating_sub(4));
            let height = height.min(area.height.saturating_sub(2));
            let popup = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).block(
                    panel(title)
                        .border_style(Style::default().fg(ACCENT))
                        .padding(ratatui::widgets::Padding::horizontal(1)),
                ),
                popup,
            );
        }
    }
    fn brief_text(&mut self, width: u16) -> Text<'static> {
        self.brief_cache
            .render(self.brief(), self.profile.language, width)
    }
    fn output_text(&self) -> Text<'static> {
        let mut lines = Vec::new();
        if self.results_stale {
            lines.push(Line::styled(
                "Previous run · edited since check",
                Style::default().fg(WARNING),
            ));
        }
        if let Some(result) = &self.result {
            for check in &result.checks {
                for (label, content, color) in [
                    ("stdout", &check.stdout, INFO),
                    ("stderr", &check.stderr, WARNING),
                    ("error", &check.detail, ERROR),
                ] {
                    if !content.is_empty() {
                        lines.push(Line::styled(
                            format!("{} · {label}", check.name),
                            Style::default().fg(color).bold(),
                        ));
                        lines.extend(
                            clean_output(content)
                                .lines()
                                .map(|line| Line::raw(line.to_string())),
                        );
                        lines.push(Line::raw(""));
                    }
                }
            }
            if lines.is_empty() || (self.results_stale && lines.len() == 1) {
                lines.push(Line::styled(
                    "No stdout or stderr from the last run.",
                    Style::default().fg(MUTED),
                ));
            }
        } else {
            lines.push(Line::styled(
                "print / console.log / console.error appear here.",
                Style::default().fg(MUTED),
            ));
            lines.push(Line::raw("F5 runs code against the examples and checks."));
        }
        Text::from(lines)
    }
    fn results_text(&self) -> Text<'static> {
        if self.status_error {
            return Text::from(vec![
                Line::styled(
                    "Could not finish · your code is kept locally",
                    Style::default().fg(ERROR).bold(),
                ),
                Line::raw(self.status.clone()),
                Line::raw(""),
                Line::raw("F5 retry checks · F6 retry preparation · Esc save"),
            ]);
        }
        Text::from(
            self.results()
                .lines()
                .map(|line| {
                    let style = if line.starts_with('✓') || line.contains("Rep complete") {
                        Style::default().fg(SUCCESS)
                    } else if line.starts_with('×') || line.starts_with('!') {
                        Style::default().fg(ERROR)
                    } else if line.contains("checks passed") || line.starts_with("Previous run") {
                        Style::default().fg(WARNING).bold()
                    } else if line.starts_with("REFERENCE") {
                        Style::default().fg(INFO).bold()
                    } else {
                        Style::default().fg(INK)
                    };
                    Line::styled(line.to_owned(), style)
                })
                .collect::<Vec<_>>(),
        )
    }
    fn brief(&self) -> String {
        let Some(rep) = &self.rep else {
            return "\nYour agent sets the challenge. You write the code.\n\nPreparing one small, useful decision.\n\nGeneration uses your Codex allowance and sends only your selected profile, recent rep families and exercise feedback.\n\nIf setup is incomplete, run spar doctor or spar setup.\nF6 retries. Ctrl+Q saves and closes.".into();
        };
        let mut text = String::new();
        if let Some(a) = &self.attempt
            && a.hints > 0
        {
            text.push_str(&format!(
                "HINT {}\n{}\n\n",
                a.hints,
                rep.hints[a.hints.min(rep.hints.len()) - 1]
            ));
        }
        text.push_str("STARTING POINT\n");
        text.push_str(rep.mode.starting_point());
        text.push_str("\n\n");
        text.push_str(&rep.brief);
        if rep.examples.is_empty() {
            text.push_str(&format!("\n\nEXAMPLES\n{}", rep.visible_tests.trim()));
        } else {
            for (i, example) in rep.examples.iter().enumerate() {
                text.push_str(&format!(
                    "\n\nEXAMPLE {}\nInput: {}\nOutput: {}\n{}",
                    i + 1,
                    example.input_json,
                    example.output_json,
                    example.explanation
                ));
            }
        }
        text.push_str("\n\nREQUIREMENTS\n");
        for req in &rep.requirements {
            text.push_str(&format!("\n{}  {}", req.id, req.description));
        }
        if rep.checks.iter().any(|check| check.name.is_some()) {
            text.push_str("\n\nCHECK COVERAGE\nThe examples plus these behaviors are checked:\n");
            for (index, check) in rep.checks.iter().enumerate() {
                text.push_str(&format!("• {}\n", check.label(index)));
            }
        }
        text.push_str("\n\nYOUR TESTS\n");
        text.push_str(match rep.mode {
            Mode::Build => "Optional: add your own checks in Tests.\n",
            Mode::Debug => "Required: add a regression test that passes your fix and fails the original implementation.\n",
            Mode::Test => "Required: write tests that pass the provided implementation and catch every hidden buggy version.\n",
        });
        text.push_str(match rep.language {
            Language::Python => "assert solve(value) == expected\nF5 runs checks. print output appears in Output.\n",
            Language::Typescript => "assert.deepEqual(solve(value), expected);\nF5 runs checks. console.log output appears in Output.\n",
        });
        if let Some(a) = &self.attempt {
            for (i, hint) in rep.hints.iter().take(a.hints.saturating_sub(1)).enumerate() {
                text.push_str(&format!("\nHINT {}\n{}\n", i + 1, hint));
            }
        }
        text
    }
    fn results(&self) -> String {
        let mut text = if self.results_stale {
            "Previous run · edited since check\n".to_string()
        } else {
            String::new()
        };
        if let Some(result) = &self.result {
            let passed = result
                .checks
                .iter()
                .filter(|c| c.status == "passed")
                .count();
            text.push_str(&format!(
                "{} / {} checks passed{}\n",
                passed,
                result.checks.len(),
                if self.done() { " · Rep complete" } else { "" }
            ));
            let mut checks: Vec<_> = result.checks.iter().collect();
            checks.sort_by_key(|c| c.status == "passed");
            for c in checks {
                text.push_str(&format!(
                    "{}  {}{}\n",
                    match c.status.as_str() {
                        "passed" => "✓",
                        "failed" => "×",
                        _ => "!",
                    },
                    c.name,
                    if c.status == "error" {
                        " · syntax or runtime error"
                    } else {
                        ""
                    }
                ));
                if c.status != "passed"
                    && let Some(rep) = &self.rep
                {
                    for req in &rep.requirements {
                        if c.name
                            .split(|c: char| !c.is_alphanumeric())
                            .any(|part| part == req.id)
                        {
                            text.push_str(&format!("   {}\n", req.description));
                        }
                    }
                }
            }
        } else {
            text.push_str("F5 runs the checks. Keep coding with the problem in view.");
        }
        if let (Some(rep), Some(a)) = (&self.rep, &self.attempt) {
            if self.done() {
                text.push_str(&format!(
                    "\n\n{} · {}m {}s\n\n{}\n\nF6 starts your next rep.",
                    a.assistance(),
                    a.active_seconds / 60,
                    a.active_seconds % 60,
                    rep.explanation
                ));
            }
            if self.show_solution {
                text.push_str(&format!(
                    "\n\nREFERENCE IMPLEMENTATION\n\n{}\nREFERENCE TESTS\n\n{}\n{}",
                    rep.reference, rep.reference_tests, rep.explanation
                ));
            }
        }
        text
    }
}

pub fn run(store: &mut Store, profile: Profile, demo: bool) -> Result<()> {
    let mut app = App::new(profile, demo);
    app.next(store)?;
    // Grayscale still needs ANSI color sequences to set luminance and true black.
    // Keep this override inside the TUI; CLI output and child environments are unchanged.
    struct Grayscale(bool);
    impl Drop for Grayscale {
        fn drop(&mut self) {
            crossterm::style::Colored::set_ansi_color_disabled(self.0);
        }
    }
    let _grayscale = Grayscale(crossterm::style::Colored::ansi_color_disabled_memoized());
    crossterm::style::force_color_output(true);
    let mut terminal = ratatui::try_init()?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture);
            ratatui::restore();
        }
    }
    let _restore = Restore;
    execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
    let result = (|| -> Result<()> {
        loop {
            app.poll(store)?;
            let now = Instant::now();
            if app.rep.is_some()
                && !app.done()
                && app.job.is_none()
                && now.duration_since(app.last_input) < Duration::from_secs(60)
            {
                app.active += now.duration_since(app.tick);
            }
            app.tick = now;
            if app.save_at.elapsed() > Duration::from_secs(5) {
                app.save(store)?;
            }
            terminal.draw(|frame| app.draw(frame))?;
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
                        app.last_input = Instant::now();
                        match app.key(key, store, &mut terminal) {
                            Ok(true) => break,
                            Ok(false) => {}
                            Err(e) => app.error(format!("{e:#}")),
                        }
                    }
                    Event::Mouse(mouse) => {
                        app.last_input = Instant::now();
                        app.mouse(mouse);
                    }
                    Event::Paste(text) if app.overlay == Some(Overlay::Find) => {
                        let remaining = 200usize.saturating_sub(app.search.len());
                        for c in text.chars().filter(|c| !c.is_control()).take(remaining) {
                            if app.search.len() + c.len_utf8() > 200 {
                                break;
                            }
                            app.search.push(c);
                        }
                    }
                    Event::Paste(text) if app.overlay.is_none() => {
                        app.last_input = Instant::now();
                        app.paste(text);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    if let Some(job) = app.job.take() {
        job.cancel.store(true, Ordering::Relaxed);
        let _ = job.handle.join();
    }
    let saved = app.save(store);
    result.and(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Language;
    use ratatui::backend::TestBackend;

    fn preview() -> (tempfile::TempDir, Store, App) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(Some(dir.path())).unwrap();
        let p = Profile {
            name: "Preview".into(),
            language: Language::Typescript,
            experience: "Rusty".into(),
            interests: vec!["Backend".into()],
            minutes: 5,
        };
        store.save_profile(&p).unwrap();
        store
            .admit(
                &p.name,
                &service::bundled(p.language, Mode::Debug),
                "test fixture",
            )
            .unwrap();
        let mut app = App::new(p, true);
        app.next(&mut store).unwrap();
        (dir, store, app)
    }
    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn terminal_layout_renders_at_compact_and_wide_sizes() {
        let (_dir, _store, mut app) = preview();
        for (w, h) in [(68, 22), (80, 24), (100, 30), (120, 36)] {
            for focus in [Pane::Brief, Pane::Code, Pane::Tests, Pane::Results] {
                app.focus = focus;
                let screen = render(&mut app, w, h);
                assert!(screen.contains("THE CACHE THAT NEVER FORGETS"));
                for content in [
                    "Brief",
                    "Code",
                    "Tests",
                    "Results",
                    "A cache entry",
                    "export function solve",
                    "Write your own",
                    "F5 run",
                ] {
                    assert!(
                        screen.contains(content),
                        "Missing {content} at {w}x{h} focused on {focus:?}:\n{screen}"
                    );
                }
                assert!(!screen.contains("REFERENCE IMPLEMENTATION"));
                assert!(!screen.contains("HINT 1"));
            }
        }
        assert!(render(&mut app, 40, 10).contains("Spar needs a little more room"));
    }
    #[test]
    fn revealed_hints_are_persisted_and_reference_stays_out_of_editor() {
        let (_dir, mut store, mut app) = preview();
        let before = app.code.lines().join("\n");
        app.hint(&mut store).unwrap();
        assert!(app.brief().contains("HINT 1"));
        assert!(!app.brief().contains("HINT 2"));
        assert_eq!(store.attempts("Preview").unwrap()[0].hints, 1);
        app.show_solution = true;
        assert!(app.results().contains("REFERENCE IMPLEMENTATION"));
        assert_eq!(app.code.lines().join("\n"), before);
    }
    #[test]
    fn worker_result_is_not_lost_when_channel_disconnects() {
        let (_dir, mut store, mut app) = preview();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            tx.send(Message::Assessed(Ok(Evaluation {
                checks: vec![runner::CheckResult {
                    name: "R1".into(),
                    status: "passed".into(),
                    ..Default::default()
                }],
            })))
            .unwrap();
        });
        while !handle.is_finished() {
            thread::yield_now();
        }
        app.job = Some(Job {
            cancel: process::cancel_token(),
            rx,
            handle,
        });
        app.poll(&mut store).unwrap();
        assert!(app.done());
        assert_eq!(store.attempts("Preview").unwrap()[0].outcome, "passed");
        assert!(app.job.is_none());
        assert_eq!(
            app.focus,
            Pane::Code,
            "Finishing checks should not steal editor focus"
        );
    }

    #[test]
    fn brief_scrolling_preserves_code_cursor_and_editing() {
        let (_dir, _store, mut app) = preview();
        let before = app.code.lines().to_vec();
        let cursor = app.code.cursor();
        app.workspace_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        assert_eq!(app.brief_scroll, 1);
        assert_eq!(app.focus, Pane::Code);
        assert_eq!(app.code.cursor(), cursor);
        assert_eq!(app.code.lines(), before);
        app.workspace_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_ne!(app.code.lines(), before);
        let screen = render(&mut app, 120, 36);
        assert!(screen.contains("Brief") && screen.contains("Tests") && screen.contains("Results"));
    }

    #[test]
    fn mouse_focus_and_scrolling_are_independent_for_each_pane() {
        let (_dir, _store, mut app) = preview();
        render(&mut app, 120, 36);
        let code = app.code.lines().to_vec();
        let tests = app.workspace.tests;
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tests.x + 1,
            row: tests.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.focus, Pane::Tests);
        app.workspace_key(KeyEvent::new(KeyCode::Char('#'), KeyModifiers::NONE));
        assert_eq!(app.code.lines(), code);
        let brief = app.workspace.brief;
        app.mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: brief.x + 1,
            row: brief.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.brief_scroll, 3);
        assert_eq!(app.focus, Pane::Tests);
        assert_eq!(app.results_scroll, 0);
    }

    #[test]
    fn test_reps_start_in_tests_and_protect_the_visible_implementation() {
        let (_dir, _store, mut app) = preview();
        let rep = service::bundled(Language::Typescript, Mode::Test);
        let mut attempt = app.attempt.clone().unwrap();
        attempt.code = rep.starter.clone();
        attempt.tests = rep.visible_tests.clone();
        app.load(rep, attempt);
        assert_eq!(app.focus, Pane::Tests);
        let before = app.code.lines().to_vec();
        app.focus = Pane::Code;
        app.workspace_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.code.lines(), before);
        let screen = render(&mut app, 120, 36);
        assert!(
            screen.contains("read-only")
                && screen.contains("assert.deepEqual")
                && screen.contains("A cache entry")
        );
    }
}
