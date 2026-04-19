use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    app::{App, ConfirmAction, InputMode, Selection},
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
    preview_status: String,
    preview_text: String,
    footer: Vec<Action<'a>>,
    show_hints: bool,
    filter: &'a str,
    input: &'a str,
    status: &'a str,
    mode: &'a InputMode,
}

impl<'a> DrawState<'a> {
    pub fn from_app(app: &'a App) -> Self {
        let mut tree_lines = Vec::new();
        let visible = app.visible_rows();
        let multi_session = app.snapshot.sessions.len() > 1;
        for selection in visible {
            match selection {
                Selection::Session(session_idx) => {
                    let session = &app.snapshot.sessions[session_idx];
                    tree_lines.push(styled_line(
                        session.name.clone(),
                        app.selection.as_ref() == Some(&selection),
                        session.attached && multi_session,
                        true,
                    ));
                }
                Selection::Window(session_idx, window_idx) => {
                    let window = &app.snapshot.sessions[session_idx].windows[window_idx];
                    let multi_window = app.snapshot.sessions[session_idx].windows.len() > 1;
                    tree_lines.push(styled_line(
                        format!("  {}", window.name),
                        app.selection.as_ref() == Some(&selection),
                        window.active && multi_window,
                        false,
                    ));
                }
                Selection::Pane(session_idx, window_idx, pane_idx) => {
                    let pane =
                        &app.snapshot.sessions[session_idx].windows[window_idx].panes[pane_idx];
                    let zoom = if pane.zoomed { " z" } else { "" };
                    let multi_pane = app.snapshot.sessions[session_idx].windows[window_idx]
                        .panes
                        .len()
                        > 1;
                    tree_lines.push(styled_line(
                        format!("    {}{}", pane_tree_label(pane), zoom),
                        app.selection.as_ref() == Some(&selection),
                        pane.active && multi_pane,
                        false,
                    ));
                }
            }
        }

        Self {
            tree_lines,
            preview_status: preview_status(app),
            preview_text: app.preview.clone(),
            footer: app.actions(),
            show_hints: app.show_hints(),
            filter: &app.filter,
            input: &app.input,
            status: &app.status,
            mode: &app.mode,
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
        .constraints([
            Constraint::Percentage(24),
            Constraint::Length(1),
            Constraint::Percentage(76),
        ])
        .split(outer[0]);

    draw_tree(frame, main[0], state);
    draw_divider(frame, main[1]);
    draw_preview(frame, main[2], state);
    draw_footer(frame, outer[1], state);
}

fn draw_tree(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let paragraph = Paragraph::new(Text::from(state.tree_lines.clone())).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_divider(frame: &mut Frame<'_>, area: Rect) {
    let lines = vec![Line::from("│"); area.height as usize];
    let paragraph = Paragraph::new(Text::from(lines)).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    let status = Paragraph::new(state.preview_status.clone())
        .style(Style::default().bg(Color::Indexed(34)).fg(Color::Black))
        .wrap(Wrap { trim: false });
    let preview = Paragraph::new(state.preview_text.clone()).wrap(Wrap { trim: false });

    frame.render_widget(status, sections[0]);
    frame.render_widget(preview, sections[1]);
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let mut spans = Vec::new();
    if !state.status.is_empty() {
        spans.push(Span::raw(state.status.to_string()));
        spans.push(Span::raw(" "));
    }
    if state.show_hints {
        for action in &state.footer {
            spans.push(Span::styled(
                format!("{} ", action.key),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(format!("{}  ", action.label)));
        }
    }
    if let Some(message) = command_message(state) {
        if state.show_hints && !state.footer.is_empty() {
            spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::raw(message));
    }
    let paragraph = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn current_input(state: &DrawState<'_>) -> String {
    match state.mode {
        InputMode::Command => state.input.to_owned(),
        InputMode::Prompt(_) => state.input.to_owned(),
        InputMode::Filter => state.filter.to_owned(),
        _ => String::new(),
    }
}

fn command_message(state: &DrawState<'_>) -> Option<String> {
    match state.mode {
        InputMode::Normal => None,
        InputMode::Command => Some(format!(":{}", current_input(state))),
        InputMode::Filter if state.show_hints => Some(format!("filter: {}", current_input(state))),
        InputMode::Filter => Some(format!("/{}", current_input(state))),
        InputMode::Prompt(kind) => {
            Some(format!("{}: {}", prompt_label(kind), current_input(state)))
        }
        InputMode::Confirm(action) => Some(format!(" {}", confirm_label(action))),
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

fn pane_tree_label(pane: &Pane) -> String {
    let name = pane_name(pane);
    if pane.title.trim().is_empty() || name == pane.current_command {
        pane.current_command.clone()
    } else {
        name.to_owned()
    }
}

fn styled_line<'a>(content: String, selected: bool, active: bool, bold: bool) -> Line<'a> {
    let mut style = Style::default().fg(Color::DarkGray);
    if selected {
        style = style.bg(Color::Indexed(34)).fg(Color::Black);
    } else if active {
        style = style.fg(Color::White);
    }
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(content, style))
}

fn preview_status(app: &App) -> String {
    match app.selection.as_ref() {
        Some(Selection::Session(session_idx)) => {
            let session = &app.snapshot.sessions[*session_idx];
            if let Some(window) = session
                .windows
                .iter()
                .find(|window| window.active)
                .or_else(|| session.windows.first())
            {
                if let Some(pane) = window
                    .panes
                    .iter()
                    .find(|pane| pane.active)
                    .or_else(|| window.panes.first())
                {
                    return format!(
                        "{} | {} | {} | {}",
                        session.name, window.name, pane.current_command, pane.current_path
                    );
                }
                return format!("{} | {}", session.name, window.name);
            }
            session.name.clone()
        }
        Some(Selection::Window(session_idx, window_idx)) => {
            let session = &app.snapshot.sessions[*session_idx];
            let window = &session.windows[*window_idx];
            if let Some(pane) = window
                .panes
                .iter()
                .find(|pane| pane.active)
                .or_else(|| window.panes.first())
            {
                return format!(
                    "{} | {} | {} | {}",
                    session.name, window.name, pane.current_command, pane.current_path
                );
            }
            format!("{} | {}", session.name, window.name)
        }
        Some(Selection::Pane(session_idx, window_idx, pane_idx)) => {
            let session = &app.snapshot.sessions[*session_idx];
            let window = &session.windows[*window_idx];
            let pane = &window.panes[*pane_idx];
            format!(
                "{} | {} | {} | {}",
                session.name, window.name, pane.current_command, pane.current_path
            )
        }
        None => String::from("No sessions"),
    }
}
