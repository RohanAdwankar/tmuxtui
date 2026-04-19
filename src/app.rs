use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    tmux::{Snapshot, TargetKind, Tmux},
    ui::{Action, DrawState, draw},
};

const TICK_RATE: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Preview,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Filter,
    Prompt(PromptKind),
    Confirm(ConfirmAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptKind {
    NewSession,
    NewWindow { session_id: String },
    RenameSession { session_id: String },
    RenameWindow { window_id: String },
    RenamePane { pane_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    KillSession { session_id: String, name: String },
    KillWindow { window_id: String, name: String },
    KillPane { pane_id: String, name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Session(usize),
    Window(usize, usize),
    Pane(usize, usize, usize),
}

pub struct App {
    tmux: Tmux,
    pub(crate) snapshot: Snapshot,
    pub(crate) selection: Option<Selection>,
    pub(crate) focus: Focus,
    pub(crate) mode: InputMode,
    pub(crate) filter: String,
    pub(crate) input: String,
    pub(crate) preview: String,
    pub(crate) status: String,
    last_refresh: Instant,
    should_quit: bool,
    attach_target: Option<TargetKind>,
    count_prefix: Option<usize>,
    pending_g: bool,
}

impl App {
    pub fn new(tmux: Tmux) -> Self {
        Self {
            tmux,
            snapshot: Snapshot {
                sessions: Vec::new(),
            },
            selection: None,
            focus: Focus::Tree,
            mode: InputMode::Normal,
            filter: String::new(),
            input: String::new(),
            preview: String::new(),
            status: String::new(),
            last_refresh: Instant::now() - TICK_RATE,
            should_quit: false,
            attach_target: None,
            count_prefix: None,
            pending_g: false,
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<Option<TargetKind>> {
        self.tmux.has_tmux_binary()?;
        self.refresh()?;

        while !self.should_quit {
            terminal.draw(|frame| {
                let draw_state = DrawState::from_app(self);
                draw(frame, &draw_state);
            })?;

            if event::poll(TICK_RATE)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }

            if self.last_refresh.elapsed() >= TICK_RATE {
                if let Err(error) = self.refresh() {
                    self.status = error.to_string();
                }
            }
        }

        Ok(self.attach_target.clone())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        self.clear_transient_status();
        let result = match self.mode.clone() {
            InputMode::Normal => self.handle_normal(key),
            InputMode::Command => self.handle_command(key),
            InputMode::Filter => self.handle_filter(key),
            InputMode::Prompt(kind) => self.handle_prompt(key, kind),
            InputMode::Confirm(action) => self.handle_confirm(key, action),
        };

        if let Err(error) = result {
            self.status = error.to_string();
        }
    }

    fn handle_filter(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                self.filter = self.input.clone();
                self.mode = InputMode::Normal;
                self.refresh_preview()?;
            }
            _ => {
                if self.handle_text_input(key, true) {
                    self.refresh_preview()?;
                }
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let command = self.input.trim().to_owned();
                self.input.clear();
                self.mode = InputMode::Normal;
                self.execute_command(&command)?;
                self.refresh()?;
            }
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.input.clear();
            }
            _ => {
                self.handle_text_input(key, false);
            }
        }
        Ok(())
    }

    fn handle_normal(&mut self, key: KeyEvent) -> Result<()> {
        if self.pending_g {
            self.pending_g = false;
            match key.code {
                KeyCode::Char('g') => {
                    self.jump_to_index(0);
                    self.clear_count();
                    return Ok(());
                }
                _ => self.clear_count(),
            }
        }

        if let KeyCode::Char(ch) = key.code {
            if let Some(digit) = ch.to_digit(10) {
                if digit > 0 || self.count_prefix.is_some() {
                    self.push_count_digit(digit as usize);
                    return Ok(());
                }
            }
        }

        match key.code {
            KeyCode::Char('q') => {
                self.clear_count();
                self.should_quit = true;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = self.take_count() as isize;
                self.move_selection(count);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let count = self.take_count() as isize;
                self.move_selection(-count);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.clear_count();
                self.focus = Focus::Tree;
            }
            KeyCode::Char('g') => {
                self.pending_g = true;
            }
            KeyCode::Char('G') => {
                if self.count_prefix.is_some() {
                    let target = self.take_count().saturating_sub(1);
                    self.jump_to_index(target);
                } else {
                    let last = self.visible_rows().len().saturating_sub(1);
                    self.clear_count();
                    self.jump_to_index(last);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.clear_count();
                self.focus = Focus::Preview;
            }
            KeyCode::Tab => {
                self.clear_count();
                self.toggle_focus();
            }
            KeyCode::Enter => {
                self.clear_count();
                self.attach_selected()?;
            }
            KeyCode::Char('/') => {
                self.clear_count();
                self.mode = InputMode::Filter;
                self.input = self.filter.clone();
            }
            KeyCode::Char(':') => {
                self.clear_count();
                self.mode = InputMode::Command;
                self.input.clear();
            }
            KeyCode::Char('n') => {
                self.clear_count();
                self.start_create_prompt();
            }
            KeyCode::Char('o') => {
                self.clear_count();
                self.start_new_window_prompt();
            }
            KeyCode::Char('O') => {
                self.clear_count();
                self.start_create_prompt();
            }
            KeyCode::Char('r') => {
                self.clear_count();
                self.start_rename_prompt();
            }
            KeyCode::Char('x') => {
                self.clear_count();
                self.start_kill_prompt();
            }
            KeyCode::Char('s') => {
                self.clear_count();
                self.split_selected()?;
            }
            KeyCode::Char('z') => {
                self.clear_count();
                self.zoom_selected()?;
            }
            KeyCode::Char('w') => {
                self.clear_count();
                self.start_new_window_prompt();
            }
            KeyCode::Char('R') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.clear_count();
                self.refresh()?;
            }
            _ => {
                self.pending_g = false;
                self.clear_count();
            }
        }
        Ok(())
    }

    fn handle_prompt(&mut self, key: KeyEvent, kind: PromptKind) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let value = self.input.trim().to_owned();
                match kind {
                    PromptKind::NewSession => self.tmux.create_session(&value)?,
                    PromptKind::NewWindow { session_id } => {
                        let window_id = self.tmux.new_window(&session_id, &value)?;
                        self.mode = InputMode::Normal;
                        self.input.clear();
                        self.status = String::from("saved");
                        self.refresh()?;
                        self.selection = self.selection_for_window(&session_id, &window_id);
                        self.refresh_preview()?;
                        return Ok(());
                    }
                    PromptKind::RenameSession { session_id } => self
                        .tmux
                        .rename_session(&session_id, &self.default_session_name(&value))?,
                    PromptKind::RenameWindow { window_id } => self
                        .tmux
                        .rename_window(&window_id, &self.default_window_name(&window_id, &value))?,
                    PromptKind::RenamePane { pane_id } => {
                        self.tmux.rename_pane(&pane_id, &value)?
                    }
                }
                self.mode = InputMode::Normal;
                self.input.clear();
                self.status = String::from("saved");
                self.refresh()?;
            }
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.input.clear();
            }
            _ => {
                self.handle_text_input(key, false);
            }
        }
        Ok(())
    }

    fn handle_confirm(&mut self, key: KeyEvent, action: ConfirmAction) -> Result<()> {
        match key.code {
            KeyCode::Char('y') => {
                match action {
                    ConfirmAction::KillSession { session_id, .. } => {
                        self.tmux.kill_session(&session_id)?
                    }
                    ConfirmAction::KillWindow { window_id, .. } => {
                        self.tmux.kill_window(&window_id)?
                    }
                    ConfirmAction::KillPane { pane_id, .. } => self.tmux.kill_pane(&pane_id)?,
                }
                self.mode = InputMode::Normal;
                self.status = String::from("removed");
                self.refresh()?;
            }
            KeyCode::Char('n') | KeyCode::Esc => self.mode = InputMode::Normal,
            _ => {}
        }
        Ok(())
    }

    fn handle_text_input(&mut self, key: KeyEvent, is_filter: bool) -> bool {
        match key.code {
            KeyCode::Char(ch) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return false;
                }
                self.input.push(ch);
                if is_filter {
                    self.filter = self.input.clone();
                    self.reconcile_selection();
                }
                true
            }
            KeyCode::Backspace => {
                self.input.pop();
                if is_filter {
                    self.filter = self.input.clone();
                    self.reconcile_selection();
                }
                true
            }
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.input.clear();
                if is_filter {
                    self.filter.clear();
                    self.reconcile_selection();
                }
                true
            }
            _ => false,
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Tree => Focus::Preview,
            Focus::Preview => Focus::Tree,
        };
    }

    fn start_create_prompt(&mut self) {
        self.input.clear();
        self.mode = InputMode::Prompt(PromptKind::NewSession);
    }

    fn start_new_window_prompt(&mut self) {
        if let Some(selection) = self.selection.clone() {
            let session_id = match selection {
                Selection::Session(session_idx) => self
                    .snapshot
                    .sessions
                    .get(session_idx)
                    .map(|session| session.id.clone()),
                Selection::Window(session_idx, _) | Selection::Pane(session_idx, _, _) => self
                    .snapshot
                    .sessions
                    .get(session_idx)
                    .map(|session| session.id.clone()),
            };
            if let Some(session_id) = session_id {
                self.input.clear();
                self.mode = InputMode::Prompt(PromptKind::NewWindow { session_id });
            }
        }
    }

    fn start_rename_prompt(&mut self) {
        self.input.clear();
        self.mode = match self.selection.clone() {
            Some(Selection::Session(session_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .map(|session| {
                    self.input = session.name.clone();
                    InputMode::Prompt(PromptKind::RenameSession {
                        session_id: session.id.clone(),
                    })
                })
                .unwrap_or(InputMode::Normal),
            Some(Selection::Window(session_idx, window_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .and_then(|session| session.windows.get(window_idx))
                .map(|window| {
                    self.input = window.name.clone();
                    InputMode::Prompt(PromptKind::RenameWindow {
                        window_id: window.id.clone(),
                    })
                })
                .unwrap_or(InputMode::Normal),
            Some(Selection::Pane(session_idx, window_idx, pane_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .and_then(|session| session.windows.get(window_idx))
                .and_then(|window| window.panes.get(pane_idx))
                .map(|pane| {
                    self.input = pane_label(pane);
                    InputMode::Prompt(PromptKind::RenamePane {
                        pane_id: pane.id.clone(),
                    })
                })
                .unwrap_or(InputMode::Normal),
            None => InputMode::Normal,
        };
    }

    fn start_kill_prompt(&mut self) {
        self.mode = match self.selection.clone() {
            Some(Selection::Session(session_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .map(|session| {
                    InputMode::Confirm(ConfirmAction::KillSession {
                        session_id: session.id.clone(),
                        name: session.name.clone(),
                    })
                })
                .unwrap_or(InputMode::Normal),
            Some(Selection::Window(session_idx, window_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .and_then(|session| session.windows.get(window_idx))
                .map(|window| {
                    InputMode::Confirm(ConfirmAction::KillWindow {
                        window_id: window.id.clone(),
                        name: window.name.clone(),
                    })
                })
                .unwrap_or(InputMode::Normal),
            Some(Selection::Pane(session_idx, window_idx, pane_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .and_then(|session| session.windows.get(window_idx))
                .and_then(|window| window.panes.get(pane_idx))
                .map(|pane| {
                    InputMode::Confirm(ConfirmAction::KillPane {
                        pane_id: pane.id.clone(),
                        name: pane_label(pane),
                    })
                })
                .unwrap_or(InputMode::Normal),
            None => InputMode::Normal,
        };
    }

    fn split_selected(&mut self) -> Result<()> {
        if let Some(pane_id) = self.selected_pane_id() {
            self.tmux.split_pane(&pane_id)?;
            self.refresh()?;
        }
        Ok(())
    }

    fn zoom_selected(&mut self) -> Result<()> {
        if let Some(pane_id) = self.selected_pane_id() {
            self.tmux.toggle_zoom(&pane_id)?;
            self.refresh()?;
        }
        Ok(())
    }

    fn attach_selected(&mut self) -> Result<()> {
        if let Some(target) = self.selected_target() {
            if let Some(session_id) = self.selected_session_id() {
                self.tmux.set_last_session(&session_id)?;
            }
            self.attach_target = Some(target);
            self.should_quit = true;
        }
        Ok(())
    }

    fn execute_command(&mut self, command: &str) -> Result<()> {
        match command {
            "q" => {
                self.should_quit = true;
            }
            "hidehints" => {
                self.tmux.set_show_hints(false)?;
                self.status = String::from("hints hidden");
            }
            "showhints" => {
                self.tmux.set_show_hints(true)?;
                self.status = String::from("hints shown");
            }
            "hidestatus" => {
                self.tmux.set_show_status(false)?;
                self.status = String::from("tmux status hidden");
            }
            "showstatus" | "showstus" => {
                self.tmux.set_show_status(true)?;
                self.status = String::from("tmux status shown");
            }
            _ if command.starts_with("sidebar ") => {
                let value = command["sidebar ".len()..].trim();
                match value.parse::<u8>() {
                    Ok(percent) => {
                        self.tmux.set_sidebar_percent(percent)?;
                        self.status = format!("sidebar {}", percent.min(100));
                    }
                    Err(_) => {
                        self.status = String::from("sidebar expects 0-100");
                    }
                }
            }
            "" => {}
            _ => {
                self.status = format!("unknown command: {command}");
            }
        }
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        self.snapshot = self.tmux.snapshot()?;
        self.reconcile_selection();
        self.refresh_preview()?;
        self.last_refresh = Instant::now();
        Ok(())
    }

    fn reconcile_selection(&mut self) {
        let visible = self.visible_rows();
        if visible.is_empty() {
            self.selection = None;
            return;
        }

        if let Some(current) = &self.selection {
            if visible.iter().any(|item| *item == *current) {
                return;
            }
        }
        self.selection = self
            .preferred_selection()
            .filter(|selection| visible.iter().any(|item| item == selection))
            .or_else(|| visible.first().cloned());
    }

    fn refresh_preview(&mut self) -> Result<()> {
        self.preview = if let Some(pane_id) = self.selected_pane_id() {
            self.tmux.capture_pane(&pane_id)?
        } else {
            String::from("No pane selected")
        };
        Ok(())
    }

    fn move_selection(&mut self, delta: isize) {
        let visible = self.visible_rows();
        if visible.is_empty() {
            self.selection = None;
            return;
        }

        let current = self
            .selection
            .as_ref()
            .and_then(|selection| visible.iter().position(|item| item == selection))
            .unwrap_or(0);

        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            (current + delta as usize).min(visible.len().saturating_sub(1))
        };
        self.selection = Some(visible[next].clone());
    }

    fn jump_to_index(&mut self, target: usize) {
        let visible = self.visible_rows();
        if visible.is_empty() {
            self.selection = None;
            return;
        }

        let index = target.min(visible.len().saturating_sub(1));
        self.selection = Some(visible[index].clone());
    }

    pub(crate) fn visible_rows(&self) -> Vec<Selection> {
        let needle = self.filter.to_lowercase();
        let mut rows = Vec::new();
        for (session_idx, session) in self.snapshot.sessions.iter().enumerate() {
            let session_match = self.matches_filter(&session.name, &needle);
            if session_match {
                rows.push(Selection::Session(session_idx));
            }

            let show_windows = self.should_show_windows(session);
            for (window_idx, window) in session.windows.iter().enumerate() {
                let window_match = self.matches_filter(&window.name, &needle);
                if show_windows && (session_match || window_match) {
                    rows.push(Selection::Window(session_idx, window_idx));
                }

                let show_panes = self.should_show_panes(session, window);
                for (pane_idx, pane) in window.panes.iter().enumerate() {
                    let pane_match = self.matches_filter(&pane_label(pane), &needle)
                        || self.matches_filter(&pane.current_command, &needle)
                        || self.matches_filter(&pane.current_path, &needle);
                    if show_panes && (session_match || window_match || pane_match) {
                        rows.push(Selection::Pane(session_idx, window_idx, pane_idx));
                    }
                }
            }
        }
        rows
    }

    fn matches_filter(&self, haystack: &str, needle: &str) -> bool {
        needle.is_empty() || haystack.to_lowercase().contains(needle)
    }

    fn preferred_selection(&self) -> Option<Selection> {
        let attached_session_idx = self
            .snapshot
            .sessions
            .iter()
            .position(|session| session.attached);
        let session_idx = attached_session_idx
            .or_else(|| {
                self.tmux.last_session().and_then(|session_id| {
                    self.snapshot
                        .sessions
                        .iter()
                        .position(|session| session.id == session_id)
                })
            })
            .or_else(|| (!self.snapshot.sessions.is_empty()).then_some(0))?;
        let session = self.snapshot.sessions.get(session_idx)?;

        let window_idx = session
            .windows
            .iter()
            .position(|window| window.active)
            .or_else(|| (!session.windows.is_empty()).then_some(0));

        if let Some(window_idx) = window_idx {
            let window = &session.windows[window_idx];
            if self.should_show_windows(session) {
                return Some(Selection::Window(session_idx, window_idx));
            }

            if self.should_show_panes(session, window) {
                let pane_idx = window
                    .panes
                    .iter()
                    .position(|pane| pane.active)
                    .or_else(|| (!window.panes.is_empty()).then_some(0))?;
                return Some(Selection::Pane(session_idx, window_idx, pane_idx));
            }
        }

        Some(Selection::Session(session_idx))
    }

    fn selection_for_window(&self, session_id: &str, window_id: &str) -> Option<Selection> {
        self.selection_for_ids(session_id, window_id, "")
    }

    fn selected_session_id(&self) -> Option<String> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx)
            | Selection::Window(session_idx, _)
            | Selection::Pane(session_idx, _, _) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .map(|session| session.id.clone()),
        }
    }

    fn selection_for_ids(
        &self,
        session_id: &str,
        window_id: &str,
        pane_id: &str,
    ) -> Option<Selection> {
        let session_idx = self
            .snapshot
            .sessions
            .iter()
            .position(|session| session.id == session_id)?;
        let session = self.snapshot.sessions.get(session_idx)?;
        let window_idx = session
            .windows
            .iter()
            .position(|window| window.id == window_id)?;
        let window = session.windows.get(window_idx)?;

        if self.should_show_panes(session, window) {
            let pane_idx = if pane_id.is_empty() {
                window.panes.iter().position(|pane| pane.active)
            } else {
                window.panes.iter().position(|pane| pane.id == pane_id)
            }
            .or_else(|| (!window.panes.is_empty()).then_some(0))?;
            return Some(Selection::Pane(session_idx, window_idx, pane_idx));
        }

        if self.should_show_windows(session) {
            return Some(Selection::Window(session_idx, window_idx));
        }

        Some(Selection::Session(session_idx))
    }

    fn should_show_windows(&self, session: &crate::tmux::Session) -> bool {
        session.windows.len() > 1
    }

    fn should_show_panes(
        &self,
        _session: &crate::tmux::Session,
        window: &crate::tmux::Window,
    ) -> bool {
        window.panes.len() > 1
    }

    fn selected_target(&self) -> Option<TargetKind> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .map(|session| TargetKind::Session(session.id.clone())),
            Selection::Window(session_idx, window_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .map(|window| TargetKind::Window {
                    session_id: self.snapshot.sessions[*session_idx].id.clone(),
                    window_id: window.id.clone(),
                }),
            Selection::Pane(session_idx, window_idx, pane_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .and_then(|window| window.panes.get(*pane_idx))
                .map(|pane| TargetKind::Pane {
                    session_id: self.snapshot.sessions[*session_idx].id.clone(),
                    window_id: self.snapshot.sessions[*session_idx].windows[*window_idx]
                        .id
                        .clone(),
                    pane_id: pane.id.clone(),
                }),
        }
    }

    fn selected_pane_id(&self) -> Option<String> {
        match self.selection.as_ref()? {
            Selection::Pane(session_idx, window_idx, pane_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .and_then(|window| window.panes.get(*pane_idx))
                .map(|pane| pane.id.clone()),
            Selection::Window(session_idx, window_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .and_then(|window| {
                    window
                        .panes
                        .iter()
                        .find(|pane| pane.active)
                        .or_else(|| window.panes.first())
                })
                .map(|pane| pane.id.clone()),
            Selection::Session(session_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| {
                    session
                        .windows
                        .iter()
                        .find(|window| window.active)
                        .or_else(|| session.windows.first())
                })
                .and_then(|window| {
                    window
                        .panes
                        .iter()
                        .find(|pane| pane.active)
                        .or_else(|| window.panes.first())
                })
                .map(|pane| pane.id.clone()),
        }
    }

    pub fn actions(&self) -> Vec<Action<'static>> {
        if !self.tmux.show_hints() {
            return Vec::new();
        }

        let mut actions = vec![
            Action::new("enter", "attach"),
            Action::new(":", "command"),
            Action::new("j/k", "move"),
            Action::new("/", "filter"),
            Action::new("n", "new session"),
            Action::new("o", "new window"),
            Action::new("O", "new session"),
            Action::new("w", "new window"),
            Action::new("r", "rename"),
            Action::new("x", "kill"),
            Action::new("s", "split"),
            Action::new("z", "zoom"),
            Action::new("tab", "focus"),
            Action::new("^q", "leave tmux"),
            Action::new("q", "quit"),
        ];
        if !matches!(self.mode, InputMode::Normal) {
            actions = vec![
                Action::new("enter", "confirm"),
                Action::new("esc", "cancel"),
            ];
        }
        actions
    }

    pub fn show_hints(&self) -> bool {
        self.tmux.show_hints()
    }

    pub fn sidebar_percent(&self) -> u8 {
        self.tmux.sidebar_percent()
    }

    fn push_count_digit(&mut self, digit: usize) {
        let next = self.count_prefix.unwrap_or(0).saturating_mul(10) + digit;
        self.count_prefix = Some(next.max(1));
    }

    fn take_count(&mut self) -> usize {
        self.count_prefix.take().unwrap_or(1)
    }

    fn clear_count(&mut self) {
        self.count_prefix = None;
    }

    fn clear_transient_status(&mut self) {
        if !self.status.is_empty() {
            self.status.clear();
        }
    }

    fn default_session_name(&self, value: &str) -> String {
        if !value.is_empty() {
            return value.to_owned();
        }

        self.selection
            .as_ref()
            .and_then(|selection| match selection {
                Selection::Session(session_idx) => self.snapshot.sessions.get(*session_idx),
                Selection::Window(session_idx, _) | Selection::Pane(session_idx, _, _) => {
                    self.snapshot.sessions.get(*session_idx)
                }
            })
            .and_then(|session| {
                session
                    .windows
                    .iter()
                    .find(|window| window.active)
                    .or_else(|| session.windows.first())
            })
            .map(|window| window.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| String::from("session"))
    }

    fn default_window_name(&self, window_id: &str, value: &str) -> String {
        if !value.is_empty() {
            return value.to_owned();
        }

        self.snapshot
            .sessions
            .iter()
            .flat_map(|session| session.windows.iter())
            .find(|window| window.id == window_id)
            .and_then(|window| {
                window
                    .panes
                    .iter()
                    .find(|pane| pane.active)
                    .or_else(|| window.panes.first())
            })
            .map(|pane| pane.current_command.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| String::from("window"))
    }
}

fn pane_label(pane: &crate::tmux::Pane) -> String {
    if pane.title.trim().is_empty() {
        pane.id.clone()
    } else {
        pane.title.clone()
    }
}
