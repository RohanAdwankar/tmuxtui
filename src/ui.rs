use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::{
    app::{App, ConfirmAction, Focus, InputMode, Selection},
    tmux::Pane,
};

pub struct Action<'a> {
    key: &'a str,
    label: &'a str,
}

impl<'a> Action<'a> {
    pub fn new(key: &'a str, label: &'a str) -> Self {
        Self { key, label }
    }
}

pub struct DrawState<'a> {
    tree_lines: Vec<Line<'a>>,
    preview_title: String,
    preview_text: String,
    footer: Vec<Action<'a>>,
    filter: &'a str,
    input: &'a str,
    status: &'a str,
    mode: &'a InputMode,
    focus: &'a Focus,
}

impl<'a> DrawState<'a> {
    pub fn from_app(app: &'a App) -> Self {
        let mut tree_lines = Vec::new();
        let visible = app.visible_rows();
        for selection in visible {
            match selection {
                Selection::Session(session_idx) => {
                    let session = &app.snapshot.sessions[session_idx];
                    let marker = if app.selection.as_ref() == Some(&selection) {
                        ">"
                    } else {
                        " "
                    };
                    let attached = if session.attached { " *" } else { "" };
                    tree_lines.push(Line::from(vec![
                        Span::raw(marker),
                        Span::raw(" "),
                        Span::styled(&session.name, Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(attached),
                    ]));
                }
                Selection::Window(session_idx, window_idx) => {
                    let window = &app.snapshot.sessions[session_idx].windows[window_idx];
                    let marker = if app.selection.as_ref() == Some(&selection) {
                        ">"
                    } else {
                        " "
                    };
                    let active = if window.active { " *" } else { "" };
                    tree_lines.push(Line::from(format!("{marker}   {}{}", window.name, active)));
                }
                Selection::Pane(session_idx, window_idx, pane_idx) => {
                    let pane =
                        &app.snapshot.sessions[session_idx].windows[window_idx].panes[pane_idx];
                    let marker = if app.selection.as_ref() == Some(&selection) {
                        ">"
                    } else {
                        " "
                    };
                    let active = if pane.active { " *" } else { "" };
                    let zoom = if pane.zoomed { " z" } else { "" };
                    tree_lines.push(Line::from(format!(
                        "{marker}     {} · {}{}{}",
                        pane_name(pane),
                        pane.current_command,
                        active,
                        zoom
                    )));
                }
            }
        }

        let preview_title = match app.selection.as_ref() {
            Some(Selection::Session(session_idx)) => {
                format!("{} preview", app.snapshot.sessions[*session_idx].name)
            }
            Some(Selection::Window(session_idx, window_idx)) => format!(
                "{} / {}",
                app.snapshot.sessions[*session_idx].name,
                app.snapshot.sessions[*session_idx].windows[*window_idx].name
            ),
            Some(Selection::Pane(session_idx, window_idx, pane_idx)) => {
                let session = &app.snapshot.sessions[*session_idx];
                let window = &session.windows[*window_idx];
                let pane = &window.panes[*pane_idx];
                format!("{} / {} / {}", session.name, window.name, pane_name(pane))
            }
            None => String::from("No sessions"),
        };

        Self {
            tree_lines,
            preview_title,
            preview_text: app.preview.clone(),
            footer: app.actions(),
            filter: &app.filter,
            input: &app.input,
            status: &app.status,
            mode: &app.mode,
            focus: &app.focus,
        }
    }
}

pub fn draw(frame: &mut Frame<'_>, state: &DrawState<'_>) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(outer[0]);

    draw_tree(frame, main[0], state);
    draw_preview(frame, main[1], state);
    draw_footer(frame, outer[1], state);

    if !matches!(state.mode, InputMode::Normal) {
        draw_overlay(frame, state);
    }
}

fn draw_tree(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let title = match state.focus {
        Focus::Tree => "tmux",
        Focus::Preview => "tmux ",
    };
    let block = Block::default().title(title);
    let paragraph = Paragraph::new(Text::from(state.tree_lines.clone()))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let paragraph = Paragraph::new(state.preview_text.clone())
        .block(Block::default().title(state.preview_title.clone()))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let mut spans = Vec::new();
    if !state.status.is_empty() {
        spans.push(Span::raw(format!("{}   ", state.status)));
    }
    if !state.filter.is_empty() {
        spans.push(Span::raw(format!("filter:{}   ", state.filter)));
    }
    for action in &state.footer {
        spans.push(Span::styled(
            format!("{} ", action.key),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!("{}   ", action.label)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_overlay(frame: &mut Frame<'_>, state: &DrawState<'_>) {
    let area = centered_rect(60, 3, frame.area());
    frame.render_widget(Clear, area);
    let text = match state.mode {
        InputMode::Filter => format!("filter: {}", state.filter),
        InputMode::Prompt(kind) => format!("{}: {}", prompt_label(kind), current_input(state)),
        InputMode::Confirm(action) => confirm_label(action),
        InputMode::Normal => String::new(),
    };
    frame.render_widget(Paragraph::new(text).block(Block::default()), area);
}

fn current_input(state: &DrawState<'_>) -> String {
    match state.mode {
        InputMode::Prompt(_) => state.input.to_owned(),
        InputMode::Filter => state.filter.to_owned(),
        _ => String::new(),
    }
}

fn prompt_label(kind: &crate::app::PromptKind) -> &'static str {
    match kind {
        crate::app::PromptKind::NewSession => "new session",
        crate::app::PromptKind::NewWindow { .. } => "new window",
        crate::app::PromptKind::RenameSession { .. } => "rename session",
        crate::app::PromptKind::RenameWindow { .. } => "rename window",
        crate::app::PromptKind::RenamePane { .. } => "rename pane",
    }
}

fn confirm_label(action: &ConfirmAction) -> String {
    match action {
        ConfirmAction::KillSession { name, .. } => format!("kill session {name}? y/n"),
        ConfirmAction::KillWindow { name, .. } => format!("kill window {name}? y/n"),
        ConfirmAction::KillPane { name, .. } => format!("kill pane {name}? y/n"),
    }
}

fn pane_name(pane: &Pane) -> &str {
    if pane.title.trim().is_empty() {
        &pane.id
    } else {
        &pane.title
    }
}

fn centered_rect(width_percent: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}
