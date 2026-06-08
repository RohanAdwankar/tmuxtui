use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    managed_config::{key_binding_entries, KeyBindings},
    tmux::{LastTarget, Snapshot, TargetKind, Tmux},
    ui::{draw, Action, DrawState},
};

const TICK_RATE: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Filter,
    Search,
    Picker,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NormalAction {
    Quit,
    Picker,
    NextSearch,
    PreviousSearch,
    Down,
    Up,
    Top,
    Bottom,
    Kill,
    KillWindow,
    Archive,
    ArchiveWindow,
    Caffeinate,
    Cut,
    PasteChild,
    PastePeer,
    Attach,
    Search,
    Filter,
    Command,
    NewChild,
    NewPeer,
    Refresh,
    Rename,
    RemoteTmux,
    SplitDown,
    SplitRight,
    Zoom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ArchiveTarget {
    Session {
        name: String,
        panes: Vec<(String, String)>,
    },
    Window {
        name: String,
        panes: Vec<(String, String)>,
    },
    Pane {
        name: String,
        pane_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Session(usize),
    Window(usize, usize),
    Pane(usize, usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectionKey {
    session_id: String,
    window_id: Option<String>,
    pane_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CreateIntent {
    NewSession,
    NewWindow { session_id: String },
    NewPane,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CutTarget {
    Window {
        session_id: String,
        window_id: String,
        name: String,
    },
    Pane {
        window_id: String,
        pane_id: String,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PasteIntent {
    Session,
    Window {
        session_id: String,
    },
    Pane {
        session_id: String,
        window_id: String,
        pane_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PanePickerEntry {
    pub(crate) selection: Selection,
    pub(crate) label: String,
    pub(crate) preview: String,
}

pub struct App {
    tmux: Tmux,
    pub(crate) snapshot: Snapshot,
    pub(crate) selection: Option<Selection>,
    pub(crate) mode: InputMode,
    pub(crate) filter: String,
    pub(crate) search: String,
    pub(crate) input: String,
    pub(crate) preview: String,
    pub(crate) picker_entries: Vec<PanePickerEntry>,
    pub(crate) picker_index: usize,
    pub(crate) status: String,
    pub(crate) pinned_pane: Option<String>,
    pub(crate) caffeinated_targets: HashSet<String>,
    last_refresh: Instant,
    should_quit: bool,
    attach_target: Option<TargetKind>,
    cut_target: Option<CutTarget>,
    count_prefix: Option<usize>,
    pending_keys: Vec<String>,
    key_bindings: KeyBindings,
}

impl App {
    pub fn new(tmux: Tmux) -> Self {
        let key_bindings = tmux.key_bindings().clone();
        Self {
            tmux,
            snapshot: Snapshot {
                sessions: Vec::new(),
            },
            selection: None,
            mode: InputMode::Normal,
            filter: String::new(),
            search: String::new(),
            input: String::new(),
            preview: String::new(),
            picker_entries: Vec::new(),
            picker_index: 0,
            status: String::new(),
            pinned_pane: None,
            caffeinated_targets: HashSet::new(),
            last_refresh: Instant::now() - TICK_RATE,
            should_quit: false,
            attach_target: None,
            cut_target: None,
            count_prefix: None,
            pending_keys: Vec::new(),
            key_bindings,
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
            InputMode::Search => self.handle_search(key),
            InputMode::Picker => self.handle_picker(key),
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

    fn handle_search(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                self.search = self.input.clone();
                self.search_match(true, true);
                self.mode = InputMode::Normal;
                self.input.clear();
                self.refresh_preview()?;
            }
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.input.clear();
            }
            _ => {
                if self.handle_text_input(key, false) {
                    self.search = self.input.clone();
                    self.search_match(true, true);
                    self.refresh_preview()?;
                }
            }
        }
        Ok(())
    }

    fn handle_picker(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                if self.select_picker_entry() {
                    self.attach_selected()?;
                }
            }
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.input.clear();
                self.refresh_preview()?;
            }
            KeyCode::Down => self.move_picker(1),
            KeyCode::Up => self.move_picker(-1),
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_picker(1);
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_picker(-1);
            }
            _ => {
                if self.handle_text_input(key, false) {
                    self.clamp_picker_index();
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
        let Some(token) = normal_key_token(key) else {
            self.clear_count();
            self.pending_keys.clear();
            return Ok(());
        };

        if self.pending_keys.is_empty() {
            if let KeyCode::Char(ch) = key.code {
                if let Some(digit) = ch.to_digit(10) {
                    if digit > 0 || self.count_prefix.is_some() {
                        self.push_count_digit(digit as usize);
                        return Ok(());
                    }
                }
            }
        }

        self.pending_keys.push(token);
        if let Some(action) = self.pending_normal_action() {
            self.pending_keys.clear();
            return self.run_normal_action(action);
        }
        if self.has_pending_normal_prefix() {
            return Ok(());
        }
        self.pending_keys.clear();
        self.clear_count();
        Ok(())
    }

    fn pending_normal_action(&self) -> Option<NormalAction> {
        normal_bindings(&self.key_bindings)
            .into_iter()
            .find_map(|(action, binding)| {
                (binding == self.pending_keys.as_slice()).then_some(action)
            })
    }

    fn has_pending_normal_prefix(&self) -> bool {
        normal_bindings(&self.key_bindings)
            .into_iter()
            .any(|(_, binding)| starts_with(binding, &self.pending_keys))
    }

    fn run_normal_action(&mut self, action: NormalAction) -> Result<()> {
        match action {
            NormalAction::Quit => self.should_quit = true,
            NormalAction::Picker => self.start_picker()?,
            NormalAction::NextSearch => {
                self.search_match(true, false);
                self.refresh_preview()?;
            }
            NormalAction::PreviousSearch => {
                self.search_match(false, false);
                self.refresh_preview()?;
            }
            NormalAction::Down => {
                let count = self.take_count() as isize;
                self.move_selection(count);
                return Ok(());
            }
            NormalAction::Up => {
                let count = self.take_count() as isize;
                self.move_selection(-count);
                return Ok(());
            }
            NormalAction::Top => self.jump_to_index(0),
            NormalAction::Bottom => {
                if self.count_prefix.is_some() {
                    let target = self.take_count().saturating_sub(1);
                    self.jump_to_index(target);
                    return Ok(());
                }
                let last = self.visible_rows().len().saturating_sub(1);
                self.jump_to_index(last);
            }
            NormalAction::Kill => self.start_kill_prompt(false),
            NormalAction::KillWindow => self.start_kill_prompt(true),
            NormalAction::Archive => self.archive_selected(false)?,
            NormalAction::ArchiveWindow => self.archive_selected(true)?,
            NormalAction::Caffeinate => self.toggle_caffeinate_selected()?,
            NormalAction::Cut => self.cut_selected(),
            NormalAction::PasteChild => self.paste_cut(false)?,
            NormalAction::PastePeer => self.paste_cut(true)?,
            NormalAction::Attach => self.attach_selected()?,
            NormalAction::Search => {
                self.mode = InputMode::Search;
                self.input.clear();
            }
            NormalAction::Filter => {
                self.mode = InputMode::Filter;
                self.input = self.filter.clone();
            }
            NormalAction::Command => {
                self.mode = InputMode::Command;
                self.input.clear();
            }
            NormalAction::NewChild => self.start_child_create()?,
            NormalAction::NewPeer => self.start_peer_create()?,
            NormalAction::Refresh => self.refresh()?,
            NormalAction::Rename => self.start_rename_prompt(),
            NormalAction::RemoteTmux => self.attach_remote_tmux_selected()?,
            NormalAction::SplitDown => self.split_selected(false)?,
            NormalAction::SplitRight => self.split_selected(true)?,
            NormalAction::Zoom => self.zoom_selected()?,
        }
        self.clear_count();
        Ok(())
    }

    fn handle_prompt(&mut self, key: KeyEvent, kind: PromptKind) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let value = self.input.trim().to_owned();
                match kind {
                    PromptKind::NewSession => {
                        let session_id = self.tmux.create_session(&value)?;
                        self.mode = InputMode::Normal;
                        self.input.clear();
                        self.status = String::from("saved");
                        self.refresh()?;
                        self.selection = self.selection_for_session(&session_id);
                        self.refresh_preview()?;
                        return Ok(());
                    }
                    PromptKind::NewWindow { session_id } => {
                        let base_pane_id = self.selected_pane_id();
                        let window_id =
                            self.tmux
                                .new_window(&session_id, base_pane_id.as_deref(), &value)?;
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
                    let previous_selection = self.selection_key();
                    let previous_index = self.selection.as_ref().and_then(|selection| {
                        self.visible_rows()
                            .iter()
                            .position(|item| item == selection)
                    });
                    self.reconcile_selection(previous_selection.as_ref(), previous_index);
                }
                true
            }
            KeyCode::Backspace => {
                self.input.pop();
                if is_filter {
                    self.filter = self.input.clone();
                    let previous_selection = self.selection_key();
                    let previous_index = self.selection.as_ref().and_then(|selection| {
                        self.visible_rows()
                            .iter()
                            .position(|item| item == selection)
                    });
                    self.reconcile_selection(previous_selection.as_ref(), previous_index);
                }
                true
            }
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.input.clear();
                if is_filter {
                    self.filter.clear();
                    let previous_selection = self.selection_key();
                    let previous_index = self.selection.as_ref().and_then(|selection| {
                        self.visible_rows()
                            .iter()
                            .position(|item| item == selection)
                    });
                    self.reconcile_selection(previous_selection.as_ref(), previous_index);
                }
                true
            }
            _ => false,
        }
    }

    fn start_child_create(&mut self) -> Result<()> {
        if let Some(intent) = self.child_create_intent() {
            self.apply_create_intent(intent)?;
        }
        Ok(())
    }

    fn start_peer_create(&mut self) -> Result<()> {
        if let Some(intent) = self.peer_create_intent() {
            self.apply_create_intent(intent)?;
        }
        Ok(())
    }

    fn child_create_intent(&self) -> Option<CreateIntent> {
        match self.selection.clone()? {
            Selection::Session(session_idx) => {
                self.snapshot
                    .sessions
                    .get(session_idx)
                    .map(|session| CreateIntent::NewWindow {
                        session_id: session.id.clone(),
                    })
            }
            Selection::Window(_, _) | Selection::Pane(_, _, _) => {
                self.selected_pane_id().map(|_| CreateIntent::NewPane)
            }
        }
    }

    fn peer_create_intent(&self) -> Option<CreateIntent> {
        match self.selection.clone() {
            None => Some(CreateIntent::NewSession),
            Some(Selection::Session(_)) => Some(CreateIntent::NewSession),
            Some(Selection::Window(session_idx, _)) => {
                self.snapshot
                    .sessions
                    .get(session_idx)
                    .map(|session| CreateIntent::NewWindow {
                        session_id: session.id.clone(),
                    })
            }
            Some(Selection::Pane(session_idx, _, _)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .map(|session| CreateIntent::NewWindow {
                    session_id: session.id.clone(),
                }),
        }
    }

    fn apply_create_intent(&mut self, intent: CreateIntent) -> Result<()> {
        match intent {
            CreateIntent::NewSession => {
                self.input.clear();
                self.mode = InputMode::Prompt(PromptKind::NewSession);
            }
            CreateIntent::NewWindow { session_id } => {
                self.input.clear();
                self.mode = InputMode::Prompt(PromptKind::NewWindow { session_id });
            }
            CreateIntent::NewPane => {
                self.create_pane_selected()?;
            }
        }
        Ok(())
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
                    self.input = pane_label(pane_idx);
                    InputMode::Prompt(PromptKind::RenamePane {
                        pane_id: pane.id.clone(),
                    })
                })
                .unwrap_or(InputMode::Normal),
            None => InputMode::Normal,
        };
    }

    fn start_kill_prompt(&mut self, whole_window: bool) {
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
                    if !whole_window && window.panes.len() > 1 {
                        ConfirmAction::KillPane {
                            pane_id: window.panes[0].id.clone(),
                            name: format!("{} pane 1", window.name),
                        }
                    } else {
                        ConfirmAction::KillWindow {
                            window_id: window.id.clone(),
                            name: window.name.clone(),
                        }
                    }
                })
                .map(InputMode::Confirm)
                .unwrap_or(InputMode::Normal),
            Some(Selection::Pane(session_idx, window_idx, pane_idx)) => self
                .snapshot
                .sessions
                .get(session_idx)
                .and_then(|session| session.windows.get(window_idx))
                .and_then(|window| {
                    if whole_window {
                        Some(ConfirmAction::KillWindow {
                            window_id: window.id.clone(),
                            name: window.name.clone(),
                        })
                    } else {
                        window
                            .panes
                            .get(pane_idx)
                            .map(|pane| ConfirmAction::KillPane {
                                pane_id: pane.id.clone(),
                                name: pane_label(pane_idx),
                            })
                    }
                })
                .map(InputMode::Confirm)
                .unwrap_or(InputMode::Normal),
            None => InputMode::Normal,
        };
    }

    fn split_selected(&mut self, vertical: bool) -> Result<()> {
        if let Some(pane_id) = self.selected_pane_id() {
            self.tmux.split_pane(&pane_id, vertical)?;
            self.refresh()?;
        }
        Ok(())
    }

    fn create_pane_selected(&mut self) -> Result<()> {
        if let Some(base_pane_id) = self.selected_pane_id() {
            let pane_id = self.tmux.split_pane(&base_pane_id, false)?;
            self.refresh()?;
            if let Some(selection) = self.selection_for_pane(&pane_id) {
                self.selection = Some(selection);
                self.refresh_preview()?;
            }
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

    fn archive_selected(&mut self, whole_window: bool) -> Result<()> {
        let Some(target) = self.archive_target_for_selection(whole_window) else {
            self.status = String::from("nothing to archive");
            return Ok(());
        };

        let (name, panes) = match target {
            ArchiveTarget::Session { name, panes } | ArchiveTarget::Window { name, panes } => {
                (name, panes)
            }
            ArchiveTarget::Pane { name, pane_id } => (name, vec![(String::from("pane"), pane_id)]),
        };

        let path = self.tmux.archive_panes(&name, &panes)?;
        self.status = format!("archived {path}");
        Ok(())
    }

    fn toggle_caffeinate_selected(&mut self) -> Result<()> {
        let Some(target_id) = self.selected_caffeinate_target_id() else {
            self.status = String::from("caffeinate requires selection");
            return Ok(());
        };

        let enabled = self.tmux.toggle_caffeinate(&target_id)?;
        if enabled {
            self.caffeinated_targets.insert(target_id);
            self.status = String::from("caffeinated");
        } else {
            self.caffeinated_targets.remove(&target_id);
            self.status = String::from("decaffeinated");
        }
        Ok(())
    }

    fn cut_selected(&mut self) {
        self.cut_target = self.cut_target_for_selection();
        self.status = match self.cut_target.as_ref() {
            Some(CutTarget::Window { name, .. }) => format!("cut window {name}"),
            Some(CutTarget::Pane { name, .. }) => format!("cut pane {name}"),
            None => String::from("cut requires window or pane selection"),
        };
    }

    fn paste_cut(&mut self, peer: bool) -> Result<()> {
        let Some(cut_target) = self.cut_target.clone() else {
            self.status = String::from("nothing cut");
            return Ok(());
        };
        let Some(intent) = self.paste_intent(peer) else {
            self.status = String::from("paste requires selection");
            return Ok(());
        };

        match (cut_target, intent) {
            (
                CutTarget::Window {
                    session_id,
                    window_id,
                    ..
                },
                intent,
            ) => match intent {
                PasteIntent::Session => {
                    let new_session_id = self.tmux.move_window_to_new_session(&window_id)?;
                    self.cut_target = None;
                    self.refresh()?;
                    self.selection = self.selection_for_session(&new_session_id);
                    self.status = String::from("pasted session");
                    self.refresh_preview()?;
                }
                PasteIntent::Window {
                    session_id: target_session_id,
                } => {
                    if target_session_id == session_id {
                        self.status = String::from("window already in session");
                        return Ok(());
                    }
                    self.tmux
                        .move_window_to_session(&window_id, &target_session_id)?;
                    self.cut_target = None;
                    self.refresh()?;
                    self.selection = self.selection_for_window(&target_session_id, &window_id);
                    self.status = String::from("pasted window");
                    self.refresh_preview()?;
                }
                PasteIntent::Pane { .. } => {
                    self.status = String::from("window can paste as session or window");
                }
            },
            (
                CutTarget::Pane {
                    window_id, pane_id, ..
                },
                intent,
            ) => match intent {
                PasteIntent::Session => {
                    let session_id = self.tmux.move_pane_to_new_session(&pane_id)?;
                    self.cut_target = None;
                    self.refresh()?;
                    self.selection = self.selection_for_session(&session_id);
                    self.status = String::from("pasted session");
                    self.refresh_preview()?;
                }
                PasteIntent::Window { session_id } => {
                    let window_id = self.tmux.move_pane_to_new_window(&pane_id, &session_id)?;
                    self.cut_target = None;
                    self.refresh()?;
                    self.selection = self.selection_for_window(&session_id, &window_id);
                    self.status = String::from("pasted window");
                    self.refresh_preview()?;
                }
                PasteIntent::Pane {
                    session_id: target_session_id,
                    window_id: target_window_id,
                    pane_id: target_pane_id,
                } => {
                    if target_window_id == window_id {
                        self.status = String::from("pane already in window");
                        return Ok(());
                    }
                    self.tmux.move_pane_to_window(&pane_id, &target_pane_id)?;
                    self.cut_target = None;
                    self.refresh()?;
                    self.selection =
                        self.selection_for_ids(&target_session_id, &target_window_id, &pane_id);
                    self.status = String::from("pasted pane");
                    self.refresh_preview()?;
                }
            },
        }
        Ok(())
    }

    fn attach_selected(&mut self) -> Result<()> {
        if let Some(target) = self.selected_target() {
            let last_target = self
                .last_target_for_selection()
                .unwrap_or_else(|| target.clone());
            self.tmux.set_last_target(&last_target)?;
            self.attach_target = Some(target);
            self.should_quit = true;
        }
        Ok(())
    }

    fn attach_remote_tmux_selected(&mut self) -> Result<()> {
        let Some(pane_id) = self.selected_pane_id() else {
            self.status = String::from("remote tmux requires pane selection");
            return Ok(());
        };
        let Some(target) = self.selected_target() else {
            self.status = String::from("remote tmux requires pane selection");
            return Ok(());
        };

        let last_target = self
            .last_target_for_selection()
            .unwrap_or_else(|| target.clone());
        self.tmux.set_last_target(&last_target)?;
        self.tmux.attach_remote_tmux(&pane_id)?;
        self.attach_target = Some(target);
        self.should_quit = true;
        Ok(())
    }

    fn execute_command(&mut self, command: &str) -> Result<()> {
        match command {
            "q" => {
                self.should_quit = true;
            }
            "pin" | "p" => {
                if let Some(pane_id) = self.selected_pane_id() {
                    self.tmux.set_pinned_pane(Some(&pane_id))?;
                    self.pinned_pane = Some(pane_id);
                    self.status = String::from("pane pinned");
                } else {
                    self.status = String::from("pin requires pane selection");
                }
            }
            "unpin" | "up" => {
                self.tmux.set_pinned_pane(None)?;
                self.pinned_pane = None;
                self.status = String::from("pane unpinned");
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
                if value == "a" {
                    self.tmux.set_sidebar_auto()?;
                    self.status = String::from("sidebar auto");
                } else {
                    match value.parse::<u8>() {
                        Ok(percent) => {
                            self.tmux.set_sidebar_percent(percent)?;
                            self.status = format!("sidebar {}", percent.min(100));
                        }
                        Err(_) => {
                            self.status = String::from("sidebar expects a or 0-100");
                        }
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
        let previous_selection = self.selection_key();
        let previous_index = self.selection.as_ref().and_then(|selection| {
            self.visible_rows()
                .iter()
                .position(|item| item == selection)
        });
        self.snapshot = self.tmux.snapshot()?;
        self.pinned_pane = self.tmux.pinned_pane();
        self.caffeinated_targets = self.tmux.caffeinated_target_ids()?.into_iter().collect();
        self.reconcile_selection(previous_selection.as_ref(), previous_index);
        self.refresh_preview()?;
        self.last_refresh = Instant::now();
        Ok(())
    }

    fn reconcile_selection(
        &mut self,
        previous_selection: Option<&SelectionKey>,
        previous_index: Option<usize>,
    ) {
        let visible = self.visible_rows();
        if visible.is_empty() {
            self.selection = None;
            return;
        }

        if let Some(selection) = previous_selection
            .and_then(|selection| self.selection_from_key(selection))
            .filter(|selection| visible.iter().any(|item| item == selection))
        {
            self.selection = Some(selection);
            return;
        }
        if let Some(selection) = previous_selection
            .and_then(|selection| self.selection_in_previous_session(selection))
            .filter(|selection| visible.iter().any(|item| item == selection))
        {
            self.selection = Some(selection);
            return;
        }
        if previous_selection.is_some() {
            if let Some(selection) = self
                .selection_adjacent_to_removed_session(previous_index, &visible)
                .filter(|selection| visible.iter().any(|item| item == selection))
            {
                self.selection = Some(selection);
                return;
            }
        }
        if let Some(index) = previous_index {
            if let Some(selection) = visible.get(index).or_else(|| visible.last()).cloned() {
                self.selection = Some(selection);
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

    fn start_picker(&mut self) -> Result<()> {
        self.input.clear();
        self.picker_index = 0;
        self.picker_entries = self.capture_picker_entries()?;
        self.mode = InputMode::Picker;
        if self.picker_entries.is_empty() {
            self.status = String::from("no panes");
        }
        Ok(())
    }

    fn capture_picker_entries(&self) -> Result<Vec<PanePickerEntry>> {
        let mut entries = Vec::new();
        for (session_idx, session) in self.snapshot.sessions.iter().enumerate() {
            for (window_idx, window) in session.windows.iter().enumerate() {
                for (pane_idx, pane) in window.panes.iter().enumerate() {
                    entries.push(PanePickerEntry {
                        selection: if pane_idx == 0 && self.should_show_windows(session) {
                            Selection::Window(session_idx, window_idx)
                        } else {
                            Selection::Pane(session_idx, window_idx, pane_idx)
                        },
                        label: format!(
                            "{} / {} / {}",
                            session.name,
                            window.name,
                            pane_label(pane_idx)
                        ),
                        preview: self.tmux.capture_pane(&pane.id)?,
                    });
                }
            }
        }
        Ok(entries)
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

    fn move_picker(&mut self, delta: isize) {
        let len = self.filtered_picker_entries().len();
        if len == 0 {
            self.picker_index = 0;
            return;
        }
        self.picker_index = if delta.is_negative() {
            self.picker_index.saturating_sub(delta.unsigned_abs())
        } else {
            (self.picker_index + delta as usize).min(len.saturating_sub(1))
        };
    }

    fn clamp_picker_index(&mut self) {
        let len = self.filtered_picker_entries().len();
        self.picker_index = self.picker_index.min(len.saturating_sub(1));
    }

    pub(crate) fn filtered_picker_entries(&self) -> Vec<&PanePickerEntry> {
        let needle = self.input.to_lowercase();
        self.picker_entries
            .iter()
            .filter(|entry| {
                needle.is_empty()
                    || entry.label.to_lowercase().contains(&needle)
                    || entry.preview.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub(crate) fn selected_picker_entry(&self) -> Option<&PanePickerEntry> {
        self.filtered_picker_entries()
            .get(self.picker_index)
            .copied()
    }

    fn select_picker_entry(&mut self) -> bool {
        let Some(entry) = self.selected_picker_entry().cloned() else {
            return false;
        };
        self.selection = Some(entry.selection);
        self.mode = InputMode::Normal;
        self.input.clear();
        self.preview = entry.preview;
        true
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

    fn search_match(&mut self, forward: bool, include_current: bool) {
        if self.search.is_empty() {
            return;
        }

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
        let needle = self.search.to_lowercase();

        let iter = usize::from(!include_current)..=visible.len();
        for offset in iter {
            let index = if forward {
                (current + offset) % visible.len()
            } else {
                (current + visible.len() - (offset % visible.len())) % visible.len()
            };
            if self.row_matches_search(&visible[index], &needle) {
                self.selection = Some(visible[index].clone());
                break;
            }
        }
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
                let window_match = self.matches_filter(&window_tree_label(window), &needle);
                if show_windows && (session_match || window_match) {
                    rows.push(Selection::Window(session_idx, window_idx));
                }

                let show_panes = self.should_show_panes(session, window);
                for (pane_idx, pane) in window.panes.iter().enumerate().skip(1) {
                    let pane_match = self.matches_filter(&pane_label(pane_idx), &needle)
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

    fn matches_search(&self, haystack: &str, needle: &str) -> bool {
        needle.is_empty() || haystack.to_lowercase().starts_with(needle)
    }

    fn row_matches_search(&self, selection: &Selection, needle: &str) -> bool {
        match selection {
            Selection::Session(session_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .map(|session| self.matches_search(&session.name, needle))
                .unwrap_or(false),
            Selection::Window(session_idx, window_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .map(|window| self.matches_search(&window_tree_label(window), needle))
                .unwrap_or(false),
            Selection::Pane(session_idx, window_idx, pane_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .and_then(|window| window.panes.get(*pane_idx))
                .map(|_| self.matches_search(&pane_label(*pane_idx), needle))
                .unwrap_or(false),
        }
    }

    fn preferred_selection(&self) -> Option<Selection> {
        if let Some(target) = self.tmux.last_target() {
            if let Some(selection) = self.selection_from_last_target(&target) {
                return Some(selection);
            }

            if let Some(session_idx) = self
                .snapshot
                .sessions
                .iter()
                .position(|session| session.attached)
            {
                return Some(Selection::Session(session_idx));
            }

            if !self.snapshot.sessions.is_empty() {
                return Some(Selection::Session(0));
            }
        }

        let attached_session_idx = self
            .snapshot
            .sessions
            .iter()
            .position(|session| session.attached);
        let session_idx =
            attached_session_idx.or_else(|| (!self.snapshot.sessions.is_empty()).then_some(0))?;
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

    fn selection_for_pane(&self, pane_id: &str) -> Option<Selection> {
        self.snapshot
            .sessions
            .iter()
            .flat_map(|session| session.windows.iter())
            .find(|window| window.panes.iter().any(|pane| pane.id == pane_id))
            .and_then(|window| self.selection_for_ids(&window.session_id, &window.id, pane_id))
    }

    fn selection_for_session(&self, session_id: &str) -> Option<Selection> {
        self.snapshot
            .sessions
            .iter()
            .position(|session| session.id == session_id)
            .map(Selection::Session)
    }

    fn selection_from_last_target(&self, target: &LastTarget) -> Option<Selection> {
        if let Some(window_id) = target.window_id.as_deref() {
            return self.selection_for_ids(
                &target.session_id,
                window_id,
                target.pane_id.as_deref().unwrap_or(""),
            );
        }

        self.snapshot
            .sessions
            .iter()
            .position(|session| session.id == target.session_id)
            .map(Selection::Session)
    }

    fn selection_key(&self) -> Option<SelectionKey> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx) => {
                self.snapshot
                    .sessions
                    .get(*session_idx)
                    .map(|session| SelectionKey {
                        session_id: session.id.clone(),
                        window_id: None,
                        pane_id: None,
                    })
            }
            Selection::Window(session_idx, window_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| {
                    session.windows.get(*window_idx).map(|window| SelectionKey {
                        session_id: session.id.clone(),
                        window_id: Some(window.id.clone()),
                        pane_id: None,
                    })
                }),
            Selection::Pane(session_idx, window_idx, pane_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| {
                    session.windows.get(*window_idx).and_then(|window| {
                        window.panes.get(*pane_idx).map(|pane| SelectionKey {
                            session_id: session.id.clone(),
                            window_id: Some(window.id.clone()),
                            pane_id: Some(pane.id.clone()),
                        })
                    })
                }),
        }
    }

    fn selection_from_key(&self, selection: &SelectionKey) -> Option<Selection> {
        if let Some(window_id) = selection.window_id.as_deref() {
            if selection.pane_id.is_none() {
                let session_idx = self
                    .snapshot
                    .sessions
                    .iter()
                    .position(|session| session.id == selection.session_id)?;
                let session = self.snapshot.sessions.get(session_idx)?;
                let window_idx = session
                    .windows
                    .iter()
                    .position(|window| window.id == window_id)?;

                if self.should_show_windows(session) {
                    return Some(Selection::Window(session_idx, window_idx));
                }
            }
            return self.selection_for_ids(
                &selection.session_id,
                window_id,
                selection.pane_id.as_deref().unwrap_or(""),
            );
        }

        self.snapshot
            .sessions
            .iter()
            .position(|session| session.id == selection.session_id)
            .map(Selection::Session)
    }

    fn selection_in_previous_session(&self, selection: &SelectionKey) -> Option<Selection> {
        let session_idx = self
            .snapshot
            .sessions
            .iter()
            .position(|session| session.id == selection.session_id)?;
        let session = self.snapshot.sessions.get(session_idx)?;

        if let Some(window_id) = selection.window_id.as_deref() {
            if let Some(window_idx) = session
                .windows
                .iter()
                .position(|window| window.id == window_id)
            {
                let window = session.windows.get(window_idx)?;
                if selection.pane_id.is_none() && self.should_show_windows(session) {
                    return Some(Selection::Window(session_idx, window_idx));
                }
                if self.should_show_panes(session, window) {
                    let pane_idx = selection
                        .pane_id
                        .as_deref()
                        .and_then(|pane_id| window.panes.iter().position(|pane| pane.id == pane_id))
                        .or_else(|| window.panes.iter().position(|pane| pane.active))
                        .or_else(|| (!window.panes.is_empty()).then_some(0))?;
                    if pane_idx == 0 {
                        return Some(Selection::Window(session_idx, window_idx));
                    }
                    return Some(Selection::Pane(session_idx, window_idx, pane_idx));
                }

                if self.should_show_windows(session) {
                    return Some(Selection::Window(session_idx, window_idx));
                }
            }

            if let Some(window_idx) = session
                .windows
                .iter()
                .position(|window| window.active)
                .or_else(|| (!session.windows.is_empty()).then_some(0))
            {
                let window = session.windows.get(window_idx)?;
                if self.should_show_panes(session, window) {
                    let pane_idx = window
                        .panes
                        .iter()
                        .position(|pane| pane.active)
                        .or_else(|| (!window.panes.is_empty()).then_some(0))?;
                    if pane_idx == 0 {
                        return Some(Selection::Window(session_idx, window_idx));
                    }
                    return Some(Selection::Pane(session_idx, window_idx, pane_idx));
                }

                if self.should_show_windows(session) {
                    return Some(Selection::Window(session_idx, window_idx));
                }
            }
        }

        Some(Selection::Session(session_idx))
    }

    fn selection_adjacent_to_removed_session(
        &self,
        previous_index: Option<usize>,
        visible: &[Selection],
    ) -> Option<Selection> {
        let start = previous_index
            .map(|index| index.min(visible.len().saturating_sub(1)))
            .unwrap_or(0);

        for offset in 0..visible.len() {
            if let Some(selection) = start
                .checked_sub(offset)
                .and_then(|index| visible.get(index))
                .filter(|selection| matches!(selection, Selection::Session(_)))
            {
                return Some(selection.clone());
            }
            if offset == 0 {
                continue;
            }
            if let Some(selection) = visible
                .get(start + offset)
                .filter(|selection| matches!(selection, Selection::Session(_)))
            {
                return Some(selection.clone());
            }
        }

        None
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
            if pane_idx == 0 {
                return Some(Selection::Window(session_idx, window_idx));
            }
            return Some(Selection::Pane(session_idx, window_idx, pane_idx));
        }

        if self.should_show_windows(session) {
            return Some(Selection::Window(session_idx, window_idx));
        }

        Some(Selection::Session(session_idx))
    }

    fn should_show_windows(&self, session: &crate::tmux::Session) -> bool {
        session.windows.len() > 1 || session.windows.iter().any(|window| window.panes.len() > 1)
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
                .and_then(|window| {
                    if window.panes.len() > 1 {
                        window.panes.first().map(|pane| TargetKind::Pane {
                            session_id: self.snapshot.sessions[*session_idx].id.clone(),
                            window_id: window.id.clone(),
                            pane_id: pane.id.clone(),
                        })
                    } else {
                        Some(TargetKind::Window {
                            session_id: self.snapshot.sessions[*session_idx].id.clone(),
                            window_id: window.id.clone(),
                        })
                    }
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

    fn cut_target_for_selection(&self) -> Option<CutTarget> {
        match self.selection.as_ref()? {
            Selection::Session(_) => None,
            Selection::Window(session_idx, window_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| {
                    session
                        .windows
                        .get(*window_idx)
                        .map(|window| CutTarget::Window {
                            session_id: session.id.clone(),
                            window_id: window.id.clone(),
                            name: window.name.clone(),
                        })
                }),
            Selection::Pane(session_idx, window_idx, pane_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .and_then(|window| {
                    window.panes.get(*pane_idx).map(|pane| CutTarget::Pane {
                        window_id: window.id.clone(),
                        pane_id: pane.id.clone(),
                        name: pane_label(*pane_idx),
                    })
                }),
        }
    }

    fn archive_target_for_selection(&self, whole_window: bool) -> Option<ArchiveTarget> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                Some(ArchiveTarget::Session {
                    name: format!("session-{}", session.name),
                    panes: session_archive_panes(session),
                })
            }
            Selection::Window(session_idx, window_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                let window = session.windows.get(*window_idx)?;
                if !whole_window && window.panes.len() > 1 {
                    return window.panes.first().map(|pane| ArchiveTarget::Pane {
                        name: format!("pane-{}-1", window.name),
                        pane_id: pane.id.clone(),
                    });
                }
                Some(ArchiveTarget::Window {
                    name: format!("window-{}-{}", session.name, window.name),
                    panes: window_archive_panes(window),
                })
            }
            Selection::Pane(session_idx, window_idx, pane_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                let window = session.windows.get(*window_idx)?;
                if whole_window {
                    return Some(ArchiveTarget::Window {
                        name: format!("window-{}-{}", session.name, window.name),
                        panes: window_archive_panes(window),
                    });
                }
                window.panes.get(*pane_idx).map(|pane| ArchiveTarget::Pane {
                    name: format!("pane-{}-{}", window.name, pane_label(*pane_idx)),
                    pane_id: pane.id.clone(),
                })
            }
        }
    }

    fn paste_intent(&self, peer: bool) -> Option<PasteIntent> {
        match (peer, self.selection.as_ref()?) {
            (false, Selection::Session(session_idx)) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .map(|session| PasteIntent::Window {
                    session_id: session.id.clone(),
                }),
            (false, Selection::Window(_, _) | Selection::Pane(_, _, _)) => self
                .selected_pane_destination()
                .map(|(session_id, window_id, pane_id)| PasteIntent::Pane {
                    session_id,
                    window_id,
                    pane_id,
                }),
            (true, Selection::Session(_)) => Some(PasteIntent::Session),
            (true, Selection::Window(session_idx, _)) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .map(|session| PasteIntent::Window {
                    session_id: session.id.clone(),
                }),
            (true, Selection::Pane(_, _, _)) => {
                self.selected_pane_destination()
                    .map(|(session_id, window_id, pane_id)| PasteIntent::Pane {
                        session_id,
                        window_id,
                        pane_id,
                    })
            }
        }
    }

    fn selected_pane_destination(&self) -> Option<(String, String, String)> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                let window = self.base_window_for_session(session)?;
                let pane = window
                    .panes
                    .iter()
                    .find(|pane| pane.active)
                    .or_else(|| window.panes.first())?;
                Some((session.id.clone(), window.id.clone(), pane.id.clone()))
            }
            Selection::Window(session_idx, window_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                let window = session.windows.get(*window_idx)?;
                let pane = window
                    .panes
                    .iter()
                    .find(|pane| pane.active)
                    .or_else(|| window.panes.first())?;
                Some((session.id.clone(), window.id.clone(), pane.id.clone()))
            }
            Selection::Pane(session_idx, window_idx, pane_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                let window = session.windows.get(*window_idx)?;
                let pane = window.panes.get(*pane_idx)?;
                Some((session.id.clone(), window.id.clone(), pane.id.clone()))
            }
        }
    }

    fn last_target_for_selection(&self) -> Option<TargetKind> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx) => {
                let session = self.snapshot.sessions.get(*session_idx)?;
                let window_id = self.base_window_for_session(session)?.id.clone();
                Some(TargetKind::Window {
                    session_id: session.id.clone(),
                    window_id,
                })
            }
            _ => self.selected_target(),
        }
    }

    fn base_window_for_session<'a>(
        &'a self,
        session: &'a crate::tmux::Session,
    ) -> Option<&'a crate::tmux::Window> {
        if let Some(window_id) = self
            .tmux
            .last_target()
            .filter(|target| target.session_id == session.id)
            .and_then(|target| target.window_id)
        {
            if let Some(window) = session.windows.iter().find(|window| window.id == window_id) {
                return Some(window);
            }
        }

        session
            .windows
            .iter()
            .find(|window| window.active)
            .or_else(|| session.windows.first())
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
                .and_then(|window| window.panes.first())
                .map(|pane| pane.id.clone()),
            Selection::Session(session_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| self.base_window_for_session(session))
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

    fn selected_caffeinate_target_id(&self) -> Option<String> {
        match self.selection.as_ref()? {
            Selection::Session(session_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .map(|session| session.id.clone()),
            Selection::Window(session_idx, window_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .map(|window| window.id.clone()),
            Selection::Pane(session_idx, window_idx, pane_idx) => self
                .snapshot
                .sessions
                .get(*session_idx)
                .and_then(|session| session.windows.get(*window_idx))
                .and_then(|window| window.panes.get(*pane_idx))
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
            Action::new("space fg", "picker"),
            Action::new("/", "search"),
            Action::new("n/N", "next/prev"),
            Action::new("f", "filter"),
            Action::new("o/O", "new child/peer"),
            Action::new("c", "caffeinate"),
            Action::new("x/p/P", "cut/paste"),
            Action::new("r", "rename"),
            Action::new("R", "remote tmux"),
            Action::new("d/D", "kill"),
            Action::new("a/A", "archive"),
            Action::new("s/S", "split"),
            Action::new("z", "zoom"),
            Action::new("^q", "leave tmux"),
            Action::new("q", "quit"),
        ];
        if !matches!(self.mode, InputMode::Normal) {
            actions = vec![
                Action::new("enter", "confirm"),
                Action::new("esc", "cancel"),
            ];
            if matches!(self.mode, InputMode::Picker) {
                actions.insert(1, Action::new("up/down", "move"));
            }
        }
        actions
    }

    pub fn show_hints(&self) -> bool {
        self.tmux.show_hints()
    }

    pub fn sidebar_percent(&self) -> u8 {
        self.tmux.sidebar_percent()
    }

    pub fn sidebar_auto(&self) -> bool {
        self.tmux.sidebar_auto()
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

fn pane_label(pane_idx: usize) -> String {
    (pane_idx + 1).to_string()
}

fn session_archive_panes(session: &crate::tmux::Session) -> Vec<(String, String)> {
    session
        .windows
        .iter()
        .flat_map(|window| {
            window.panes.iter().enumerate().map(|(pane_idx, pane)| {
                (
                    format!("window {} pane {}", window.name, pane_idx + 1),
                    pane.id.clone(),
                )
            })
        })
        .collect()
}

fn window_archive_panes(window: &crate::tmux::Window) -> Vec<(String, String)> {
    window
        .panes
        .iter()
        .enumerate()
        .map(|(pane_idx, pane)| (format!("pane {}", pane_idx + 1), pane.id.clone()))
        .collect()
}

fn window_tree_label(window: &crate::tmux::Window) -> String {
    if window.panes.len() > 1 {
        format!("{} 1", window.name)
    } else {
        window.name.clone()
    }
}

fn normal_key_token(key: KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Char(' ') => Some(String::from("space")),
        KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(format!("ctrl-{}", ch.to_ascii_lowercase()))
        }
        KeyCode::Char(ch) => Some(ch.to_string()),
        KeyCode::Enter => Some(String::from("enter")),
        KeyCode::Up => Some(String::from("up")),
        KeyCode::Down => Some(String::from("down")),
        KeyCode::Left => Some(String::from("left")),
        KeyCode::Right => Some(String::from("right")),
        KeyCode::Esc => Some(String::from("esc")),
        KeyCode::Backspace => Some(String::from("backspace")),
        _ => None,
    }
}

fn normal_bindings(keys: &KeyBindings) -> Vec<(NormalAction, &[String])> {
    key_binding_entries(keys)
        .into_iter()
        .filter_map(|(name, binding)| {
            normal_action(name).map(|action| (action, binding.as_slice()))
        })
        .collect()
}

fn normal_action(name: &str) -> Option<NormalAction> {
    Some(match name {
        "quit" => NormalAction::Quit,
        "picker" => NormalAction::Picker,
        "next_search" => NormalAction::NextSearch,
        "previous_search" => NormalAction::PreviousSearch,
        "down" => NormalAction::Down,
        "up" => NormalAction::Up,
        "top" => NormalAction::Top,
        "bottom" => NormalAction::Bottom,
        "kill" => NormalAction::Kill,
        "kill_window" => NormalAction::KillWindow,
        "archive" => NormalAction::Archive,
        "archive_window" => NormalAction::ArchiveWindow,
        "caffeinate" => NormalAction::Caffeinate,
        "cut" => NormalAction::Cut,
        "paste_child" => NormalAction::PasteChild,
        "paste_peer" => NormalAction::PastePeer,
        "attach" => NormalAction::Attach,
        "search" => NormalAction::Search,
        "filter" => NormalAction::Filter,
        "command" => NormalAction::Command,
        "new_child" => NormalAction::NewChild,
        "new_peer" => NormalAction::NewPeer,
        "refresh" => NormalAction::Refresh,
        "rename" => NormalAction::Rename,
        "remote_tmux" => NormalAction::RemoteTmux,
        "split_down" => NormalAction::SplitDown,
        "split_right" => NormalAction::SplitRight,
        "zoom" => NormalAction::Zoom,
        _ => return None,
    })
}

fn starts_with(binding: &[String], prefix: &[String]) -> bool {
    binding.len() > prefix.len()
        && binding
            .iter()
            .zip(prefix)
            .all(|(left, right)| left == right)
}

#[cfg(test)]
mod tests {
    use super::{
        pane_label, App, ArchiveTarget, ConfirmAction, CreateIntent, CutTarget, InputMode,
        PanePickerEntry, PasteIntent, Selection, SelectionKey,
    };
    use crate::{
        managed_config::ManagedConfig,
        tmux::{Pane, Session, Snapshot, Tmux, Window},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn reconcile_selection_tracks_window_by_id_when_indices_shift() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "alpha", true), ("@2", "beta", false)]);
        app.selection = Some(Selection::Window(0, 1));

        let previous_selection = app.selection_key();
        app.snapshot = snapshot_with_windows(&[("@2", "beta", true), ("@3", "gamma", false)]);
        app.reconcile_selection(previous_selection.as_ref(), Some(2));

        assert_eq!(app.selection, Some(Selection::Window(0, 0)));
    }

    #[test]
    fn reconcile_selection_stays_in_same_session_when_window_is_removed() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$1"),
                    name: String::from("alpha"),
                    attached: true,
                    windows: vec![Window {
                        id: String::from("@1"),
                        name: String::from("main"),
                        active: true,
                        session_id: String::from("$1"),
                        panes: vec![Pane {
                            id: String::from("%1"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp/alpha"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@1"),
                        }],
                    }],
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("beta"),
                    attached: false,
                    windows: vec![
                        Window {
                            id: String::from("@2"),
                            name: String::from("one"),
                            active: true,
                            session_id: String::from("$2"),
                            panes: vec![Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp/beta"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@2"),
                            }],
                        },
                        Window {
                            id: String::from("@3"),
                            name: String::from("two"),
                            active: false,
                            session_id: String::from("$2"),
                            panes: vec![Pane {
                                id: String::from("%3"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp/beta"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@3"),
                            }],
                        },
                    ],
                },
            ],
        };
        app.selection = Some(Selection::Window(1, 1));

        let previous_selection = app.selection_key();
        app.snapshot.sessions[1].windows.remove(1);
        app.reconcile_selection(previous_selection.as_ref(), Some(4));

        assert_eq!(app.selection, Some(Selection::Session(1)));
    }

    #[test]
    fn reconcile_selection_keeps_row_position_when_session_is_removed() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$1"),
                    name: String::from("alpha"),
                    attached: true,
                    windows: vec![
                        Window {
                            id: String::from("@1"),
                            name: String::from("main"),
                            active: true,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp/alpha"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            }],
                        },
                        Window {
                            id: String::from("@2"),
                            name: String::from("recent"),
                            active: false,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp/alpha"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@2"),
                            }],
                        },
                    ],
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("beta"),
                    attached: false,
                    windows: vec![Window {
                        id: String::from("@3"),
                        name: String::from("main"),
                        active: true,
                        session_id: String::from("$2"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp/beta"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@3"),
                        }],
                    }],
                },
            ],
        };
        app.selection = Some(Selection::Session(1));

        let previous_selection = app.selection_key();
        app.snapshot.sessions.remove(1);
        app.reconcile_selection(previous_selection.as_ref(), Some(3));

        assert_eq!(app.selection, Some(Selection::Session(0)));
    }

    #[test]
    fn reconcile_selection_uses_adjacent_session_when_window_in_removed_session_was_selected() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$1"),
                    name: String::from("alpha"),
                    attached: true,
                    windows: vec![
                        Window {
                            id: String::from("@1"),
                            name: String::from("main"),
                            active: true,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp/alpha"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            }],
                        },
                        Window {
                            id: String::from("@2"),
                            name: String::from("recent"),
                            active: false,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp/alpha"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@2"),
                            }],
                        },
                    ],
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("beta"),
                    attached: false,
                    windows: vec![Window {
                        id: String::from("@3"),
                        name: String::from("main"),
                        active: true,
                        session_id: String::from("$2"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp/beta"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@3"),
                        }],
                    }],
                },
            ],
        };
        app.selection = Some(Selection::Window(1, 0));

        let previous_selection = app.selection_key();
        app.snapshot.sessions.remove(1);
        app.reconcile_selection(previous_selection.as_ref(), Some(4));

        assert_eq!(app.selection, Some(Selection::Session(0)));
    }

    #[test]
    fn session_selection_remembers_active_window_as_last_target() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "alpha", false), ("@2", "beta", true)]);
        app.selection = Some(Selection::Session(0));

        let target = app.last_target_for_selection();

        assert!(matches!(
            target,
            Some(crate::tmux::TargetKind::Window { session_id, window_id })
                if session_id == "$1" && window_id == "@2"
        ));
    }

    #[test]
    fn child_create_on_session_targets_new_window() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "alpha", false), ("@2", "beta", true)]);
        app.selection = Some(Selection::Session(0));

        assert_eq!(
            app.child_create_intent(),
            Some(CreateIntent::NewWindow {
                session_id: String::from("$1"),
            })
        );
    }

    #[test]
    fn peer_create_on_session_targets_new_session() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "alpha", true)]);
        app.selection = Some(Selection::Session(0));

        assert_eq!(app.peer_create_intent(), Some(CreateIntent::NewSession));
    }

    #[test]
    fn peer_create_on_window_targets_new_window() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "alpha", true)]);
        app.selection = Some(Selection::Window(0, 0));

        assert_eq!(
            app.peer_create_intent(),
            Some(CreateIntent::NewWindow {
                session_id: String::from("$1"),
            })
        );
    }

    #[test]
    fn peer_create_on_pane_targets_new_window() {
        let mut app = test_app();
        app.snapshot = split_window_snapshot();
        app.selection = Some(Selection::Pane(0, 0, 1));

        assert_eq!(
            app.peer_create_intent(),
            Some(CreateIntent::NewWindow {
                session_id: String::from("$1"),
            })
        );
    }

    #[test]
    fn child_create_on_window_targets_new_pane() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "alpha", true)]);
        app.selection = Some(Selection::Window(0, 0));

        assert_eq!(app.child_create_intent(), Some(CreateIntent::NewPane));
    }

    #[test]
    fn pane_label_uses_window_local_numbers() {
        assert_eq!(pane_label(0), "1");
        assert_eq!(pane_label(1), "2");
    }

    #[test]
    fn split_window_only_lists_additional_panes_as_children() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("editor"),
                    active: true,
                    session_id: String::from("$1"),
                    panes: vec![
                        Pane {
                            id: String::from("%1"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                        Pane {
                            id: String::from("%2"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: false,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                    ],
                }],
            }],
        };

        assert_eq!(
            app.visible_rows(),
            vec![
                Selection::Session(0),
                Selection::Window(0, 0),
                Selection::Pane(0, 0, 1)
            ]
        );
    }

    #[test]
    fn slash_enters_search_mode() {
        let mut app = test_app();

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert_eq!(app.mode, InputMode::Search);
        assert!(app.input.is_empty());
    }

    #[test]
    fn slash_clears_previous_search_input() {
        let mut app = test_app();
        app.search = String::from("as");
        app.input = String::from("stale");

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert_eq!(app.mode, InputMode::Search);
        assert!(app.input.is_empty());
        assert_eq!(app.search, "as");
    }

    #[test]
    fn slash_search_selects_current_matching_visible_row() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$1"),
                    name: String::from("dev"),
                    attached: false,
                    windows: vec![
                        Window {
                            id: String::from("@1"),
                            name: String::from("editor"),
                            active: true,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            }],
                        },
                        Window {
                            id: String::from("@2"),
                            name: String::from("files"),
                            active: false,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@2"),
                            }],
                        },
                    ],
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("focus"),
                    attached: false,
                    windows: vec![Window {
                        id: String::from("@3"),
                        name: String::from("main"),
                        active: true,
                        session_id: String::from("$2"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@3"),
                        }],
                    }],
                },
            ],
        };
        app.selection = Some(Selection::Window(0, 1));

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

        assert_eq!(app.selection, Some(Selection::Window(0, 1)));
        assert_eq!(app.mode, InputMode::Search);
    }

    #[test]
    fn n_and_n_repeat_search_forward_and_backward() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$1"),
                    name: String::from("dev"),
                    attached: false,
                    windows: vec![
                        Window {
                            id: String::from("@1"),
                            name: String::from("files"),
                            active: true,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            }],
                        },
                        Window {
                            id: String::from("@2"),
                            name: String::from("focus"),
                            active: false,
                            session_id: String::from("$1"),
                            panes: vec![Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@2"),
                            }],
                        },
                    ],
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("files-2"),
                    attached: false,
                    windows: vec![Window {
                        id: String::from("@3"),
                        name: String::from("main"),
                        active: true,
                        session_id: String::from("$2"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@3"),
                        }],
                    }],
                },
            ],
        };
        app.selection = Some(Selection::Session(0));

        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.selection, Some(Selection::Window(0, 0)));
        assert_eq!(app.mode, InputMode::Normal);

        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.selection, Some(Selection::Window(0, 1)));

        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.selection, Some(Selection::Session(1)));

        app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert_eq!(app.selection, Some(Selection::Window(0, 1)));

        app.handle_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert_eq!(app.selection, Some(Selection::Window(0, 0)));
    }

    #[test]
    fn search_ignores_pane_command_and_path_text() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![
                Session {
                    id: String::from("$1"),
                    name: String::from("alpha"),
                    attached: false,
                    windows: vec![Window {
                        id: String::from("@1"),
                        name: String::from("agent"),
                        active: true,
                        session_id: String::from("$1"),
                        panes: vec![
                            Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            },
                            Pane {
                                id: String::from("%2"),
                                current_command: String::from("node"),
                                current_path: String::from("/tmp/node"),
                                active: false,
                                zoomed: false,
                                window_id: String::from("@1"),
                            },
                        ],
                    }],
                },
                Session {
                    id: String::from("$2"),
                    name: String::from("node"),
                    attached: false,
                    windows: vec![Window {
                        id: String::from("@2"),
                        name: String::from("main"),
                        active: true,
                        session_id: String::from("$2"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@2"),
                        }],
                    }],
                },
            ],
        };

        assert!(!app.row_matches_search(&Selection::Pane(0, 0, 1), "n"));
        assert!(app.row_matches_search(&Selection::Session(1), "n"));
    }

    #[test]
    fn search_matches_prefix_not_substring() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("agent"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("tools"),
                    active: true,
                    session_id: String::from("$1"),
                    panes: vec![Pane {
                        id: String::from("%1"),
                        current_command: String::from("zsh"),
                        current_path: String::from("/tmp"),
                        active: true,
                        zoomed: false,
                        window_id: String::from("@1"),
                    }],
                }],
            }],
        };

        assert!(!app.row_matches_search(&Selection::Session(0), "t"));
        assert!(!app.row_matches_search(&Selection::Session(0), "n"));
        assert!(app.row_matches_search(&Selection::Window(0, 0), "t"));
        assert!(app.row_matches_search(&Selection::Session(0), "a"));
    }

    #[test]
    fn f_enters_filter_mode() {
        let mut app = test_app();

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

        assert_eq!(app.mode, InputMode::Filter);
    }

    #[test]
    fn picker_filters_labels_and_previews() {
        let mut app = test_app();
        app.input = String::from("cargo");
        app.picker_entries = vec![
            PanePickerEntry {
                selection: Selection::Window(0, 0),
                label: String::from("dev / editor / 1 / /repo"),
                preview: String::from("vim src/main.rs"),
            },
            PanePickerEntry {
                selection: Selection::Window(0, 1),
                label: String::from("dev / build / 1 / /repo"),
                preview: String::from("cargo test"),
            },
        ];

        let matches = app.filtered_picker_entries();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].selection, Selection::Window(0, 1));
    }

    #[test]
    fn space_f_g_enters_picker_mode() {
        let mut app = test_app();

        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));

        assert_eq!(app.mode, InputMode::Picker);
    }

    #[test]
    fn picker_keeps_j_and_k_as_query_text() {
        let mut app = test_app();
        app.mode = InputMode::Picker;

        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));

        assert_eq!(app.input, "jk");
    }

    #[test]
    fn custom_picker_binding_enters_picker_mode() {
        let mut app = test_app();
        app.key_bindings.picker = vec![String::from("ctrl-p")];

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));

        assert_eq!(app.mode, InputMode::Picker);
    }

    #[test]
    fn picker_enter_selects_matching_pane() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "editor", true), ("@2", "build", false)]);
        app.mode = InputMode::Picker;
        app.input = String::from("build");
        app.picker_entries = vec![
            PanePickerEntry {
                selection: Selection::Window(0, 0),
                label: String::from("dev / editor / 1 / /repo"),
                preview: String::from("vim src/main.rs"),
            },
            PanePickerEntry {
                selection: Selection::Window(0, 1),
                label: String::from("dev / build / 1 / /repo"),
                preview: String::from("cargo test"),
            },
        ];

        assert!(app.select_picker_entry());

        assert_eq!(app.selection, Some(Selection::Window(0, 1)));
        assert_eq!(app.mode, InputMode::Normal);
        assert!(app.input.is_empty());
        assert_eq!(app.preview, "cargo test");
    }

    #[test]
    fn selection_for_first_pane_maps_to_window_row() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("editor"),
                    active: true,
                    session_id: String::from("$1"),
                    panes: vec![
                        Pane {
                            id: String::from("%1"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                        Pane {
                            id: String::from("%2"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: false,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                    ],
                }],
            }],
        };

        assert_eq!(
            app.selection_for_ids("$1", "@1", "%1"),
            Some(Selection::Window(0, 0))
        );
        assert_eq!(
            app.selection_for_ids("$1", "@1", "%2"),
            Some(Selection::Pane(0, 0, 1))
        );
    }

    #[test]
    fn selection_from_key_keeps_window_row_when_split_pane_is_active() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![
                    Window {
                        id: String::from("@1"),
                        name: String::from("editor"),
                        active: true,
                        session_id: String::from("$1"),
                        panes: vec![
                            Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: false,
                                zoomed: false,
                                window_id: String::from("@1"),
                            },
                            Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            },
                        ],
                    },
                    Window {
                        id: String::from("@2"),
                        name: String::from("shell"),
                        active: false,
                        session_id: String::from("$1"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@2"),
                        }],
                    },
                ],
            }],
        };

        assert_eq!(
            app.selection_from_key(&SelectionKey {
                session_id: String::from("$1"),
                window_id: Some(String::from("@1")),
                pane_id: None,
            }),
            Some(Selection::Window(0, 0))
        );
    }

    #[test]
    fn selection_in_previous_session_keeps_window_row_when_split_pane_is_active() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![
                    Window {
                        id: String::from("@1"),
                        name: String::from("editor"),
                        active: true,
                        session_id: String::from("$1"),
                        panes: vec![
                            Pane {
                                id: String::from("%1"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: false,
                                zoomed: false,
                                window_id: String::from("@1"),
                            },
                            Pane {
                                id: String::from("%2"),
                                current_command: String::from("zsh"),
                                current_path: String::from("/tmp"),
                                active: true,
                                zoomed: false,
                                window_id: String::from("@1"),
                            },
                        ],
                    },
                    Window {
                        id: String::from("@2"),
                        name: String::from("shell"),
                        active: false,
                        session_id: String::from("$1"),
                        panes: vec![Pane {
                            id: String::from("%3"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@2"),
                        }],
                    },
                ],
            }],
        };

        assert_eq!(
            app.selection_in_previous_session(&SelectionKey {
                session_id: String::from("$1"),
                window_id: Some(String::from("@1")),
                pane_id: None,
            }),
            Some(Selection::Window(0, 0))
        );
    }

    #[test]
    fn selection_for_session_finds_new_session_by_id() {
        let app = App {
            snapshot: Snapshot {
                sessions: vec![
                    Session {
                        id: String::from("$1"),
                        name: String::from("dev"),
                        attached: false,
                        windows: Vec::new(),
                    },
                    Session {
                        id: String::from("$2"),
                        name: String::from("fresh"),
                        attached: false,
                        windows: Vec::new(),
                    },
                ],
            },
            ..test_app()
        };

        assert_eq!(app.selection_for_session("$2"), Some(Selection::Session(1)));
    }

    #[test]
    fn split_window_row_attaches_to_first_pane() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("editor"),
                    active: true,
                    session_id: String::from("$1"),
                    panes: vec![
                        Pane {
                            id: String::from("%1"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: false,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                        Pane {
                            id: String::from("%2"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                    ],
                }],
            }],
        };
        app.selection = Some(Selection::Window(0, 0));

        assert!(matches!(
            app.selected_target(),
            Some(crate::tmux::TargetKind::Pane {
                session_id,
                window_id,
                pane_id
            }) if session_id == "$1" && window_id == "@1" && pane_id == "%1"
        ));
    }

    #[test]
    fn selection_for_pane_finds_newly_created_pane_row() {
        let mut app = test_app();
        app.snapshot = split_window_snapshot();

        assert_eq!(app.selection_for_pane("%2"), Some(Selection::Pane(0, 0, 1)));
    }

    #[test]
    fn d_on_split_window_targets_first_pane() {
        let mut app = test_app();
        app.snapshot = split_window_snapshot();
        app.selection = Some(Selection::Window(0, 0));

        app.start_kill_prompt(false);

        assert_eq!(
            app.mode,
            InputMode::Confirm(ConfirmAction::KillPane {
                pane_id: String::from("%1"),
                name: String::from("editor pane 1"),
            })
        );
    }

    #[test]
    fn uppercase_d_on_split_window_targets_full_window() {
        let mut app = test_app();
        app.snapshot = split_window_snapshot();
        app.selection = Some(Selection::Window(0, 0));

        app.start_kill_prompt(true);

        assert_eq!(
            app.mode,
            InputMode::Confirm(ConfirmAction::KillWindow {
                window_id: String::from("@1"),
                name: String::from("editor"),
            })
        );
    }

    #[test]
    fn archive_target_matches_kill_scope() {
        let mut app = test_app();
        app.snapshot = split_window_snapshot();
        app.selection = Some(Selection::Window(0, 0));

        assert_eq!(
            app.archive_target_for_selection(false),
            Some(ArchiveTarget::Pane {
                name: String::from("pane-editor-1"),
                pane_id: String::from("%1"),
            })
        );
        assert_eq!(
            app.archive_target_for_selection(true),
            Some(ArchiveTarget::Window {
                name: String::from("window-dev-editor"),
                panes: vec![
                    (String::from("pane 1"), String::from("%1")),
                    (String::from("pane 2"), String::from("%2")),
                ],
            })
        );
    }

    #[test]
    fn x_cuts_selected_window() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "editor", true), ("@2", "shell", false)]);
        app.selection = Some(Selection::Window(0, 1));

        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        assert_eq!(
            app.cut_target,
            Some(CutTarget::Window {
                session_id: String::from("$1"),
                window_id: String::from("@2"),
                name: String::from("shell"),
            })
        );
        assert_eq!(app.status, "cut window shell");
    }

    #[test]
    fn x_cuts_selected_pane() {
        let mut app = test_app();
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("editor"),
                    active: true,
                    session_id: String::from("$1"),
                    panes: vec![
                        Pane {
                            id: String::from("%1"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: false,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                        Pane {
                            id: String::from("%2"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                    ],
                }],
            }],
        };
        app.selection = Some(Selection::Pane(0, 0, 1));

        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

        assert_eq!(
            app.cut_target,
            Some(CutTarget::Pane {
                window_id: String::from("@1"),
                pane_id: String::from("%2"),
                name: String::from("2"),
            })
        );
        assert_eq!(app.status, "cut pane 2");
    }

    #[test]
    fn paste_intent_matches_create_key_hierarchy() {
        let mut app = test_app();
        app.snapshot = snapshot_with_windows(&[("@1", "editor", true)]);

        app.selection = Some(Selection::Session(0));
        assert_eq!(
            app.paste_intent(false),
            Some(PasteIntent::Window {
                session_id: String::from("$1")
            })
        );
        assert_eq!(app.paste_intent(true), Some(PasteIntent::Session));

        app.selection = Some(Selection::Window(0, 0));
        assert_eq!(
            app.paste_intent(false),
            Some(PasteIntent::Pane {
                session_id: String::from("$1"),
                window_id: String::from("@1"),
                pane_id: String::from("%@1"),
            })
        );
        assert_eq!(
            app.paste_intent(true),
            Some(PasteIntent::Window {
                session_id: String::from("$1")
            })
        );
    }

    #[test]
    fn sidebar_a_enables_auto_sidebar() {
        let mut app = test_app();

        app.execute_command("sidebar a").expect("sidebar auto");
        assert!(app.sidebar_auto());
        assert_eq!(app.status, "sidebar auto");

        app.execute_command("sidebar 20").expect("sidebar percent");
        assert!(!app.sidebar_auto());
        assert_eq!(app.sidebar_percent(), 20);
    }

    fn test_app() -> App {
        let managed = ManagedConfig::bootstrap().expect("config");
        App::new(Tmux::new(managed))
    }

    fn split_window_snapshot() -> Snapshot {
        Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("editor"),
                    active: true,
                    session_id: String::from("$1"),
                    panes: vec![
                        Pane {
                            id: String::from("%1"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: false,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                        Pane {
                            id: String::from("%2"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: String::from("@1"),
                        },
                    ],
                }],
            }],
        }
    }

    fn snapshot_with_windows(windows: &[(&str, &str, bool)]) -> Snapshot {
        Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("dev"),
                attached: false,
                windows: windows
                    .iter()
                    .map(|(id, name, active)| Window {
                        id: (*id).to_owned(),
                        name: (*name).to_owned(),
                        active: *active,
                        session_id: String::from("$1"),
                        panes: vec![Pane {
                            id: format!("%{id}"),
                            current_command: String::from("zsh"),
                            current_path: String::from("/tmp"),
                            active: true,
                            zoomed: false,
                            window_id: (*id).to_owned(),
                        }],
                    })
                    .collect(),
            }],
        }
    }
}
