use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Clear,
    widgets::{Paragraph, Wrap},
};

use crate::app::{App, ConfirmAction, InputMode, Selection};

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
    picker_lines: Vec<Line<'a>>,
    picker_preview: String,
    preview_status: String,
    preview_text: String,
    footer: Vec<Action<'a>>,
    sidebar_percent: u8,
    sidebar_auto: bool,
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
                        session_label(&session.name, app.caffeinated_targets.contains(&session.id)),
                        app.selection.as_ref() == Some(&selection),
                        session.attached && multi_session,
                        true,
                    ));
                }
                Selection::Window(session_idx, window_idx) => {
                    let window = &app.snapshot.sessions[session_idx].windows[window_idx];
                    let multi_window = app.snapshot.sessions[session_idx].windows.len() > 1;
                    let pinned = window
                        .panes
                        .first()
                        .is_some_and(|pane| app.pinned_pane.as_deref() == Some(pane.id.as_str()));
                    let caffeinated = app.caffeinated_targets.contains(&window.id);
                    tree_lines.push(styled_line(
                        marker_column(
                            format!("  {}", window_tree_label(window)),
                            pinned,
                            caffeinated,
                        ),
                        app.selection.as_ref() == Some(&selection),
                        window.active && multi_window,
                        false,
                    ));
                }
                Selection::Pane(session_idx, window_idx, pane_idx) => {
                    let window = &app.snapshot.sessions[session_idx].windows[window_idx];
                    let pane = &window.panes[pane_idx];
                    let zoom = if pane.zoomed { " z" } else { "" };
                    let multi_pane = window.panes.len() > 1;
                    let pinned = app.pinned_pane.as_deref() == Some(pane.id.as_str());
                    let caffeinated = app.caffeinated_targets.contains(&pane.id);
                    tree_lines.push(styled_line(
                        marker_column(pane_tree_line(window, pane_idx, zoom), pinned, caffeinated),
                        app.selection.as_ref() == Some(&selection),
                        pane.active && multi_pane,
                        false,
                    ));
                }
            }
        }
        let picker_entries = app.filtered_picker_entries();
        let picker_lines = picker_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                styled_line(entry.label.clone(), index == app.picker_index, false, false)
            })
            .collect();
        let picker_preview = app
            .selected_picker_entry()
            .map(|entry| entry.preview.clone())
            .unwrap_or_else(|| String::from("No matching panes"));

        Self {
            tree_lines,
            picker_lines,
            picker_preview,
            preview_status: preview_status(app),
            preview_text: app.preview.clone(),
            footer: app.actions(),
            sidebar_percent: app.sidebar_percent(),
            sidebar_auto: app.sidebar_auto(),
            show_hints: app.show_hints(),
            filter: &app.filter,
            input: &app.input,
            status: &app.status,
            mode: &app.mode,
        }
    }

    fn auto_sidebar_width(&self, area_width: u16) -> u16 {
        let widest = self
            .tree_lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.chars().count())
            .max()
            .unwrap_or(0) as u16;
        widest
            .saturating_add(1)
            .clamp(8, area_width.saturating_sub(20).max(8))
    }
}

pub fn draw(frame: &mut Frame<'_>, state: &DrawState<'_>) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(frame.area());

    if state.sidebar_auto {
        let width = state.auto_sidebar_width(outer[0].width);
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(width), Constraint::Min(1)])
            .split(outer[0]);
        draw_tree(frame, main[0], state);
        draw_preview(frame, main[1], state);
    } else {
        match state.sidebar_percent {
            0 => draw_preview(frame, outer[0], state),
            100 => draw_tree(frame, outer[0], state),
            percent => {
                let main = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(percent as u16),
                        Constraint::Percentage((100 - percent) as u16),
                    ])
                    .split(outer[0]);

                draw_tree(frame, main[0], state);
                draw_preview(frame, main[1], state);
            }
        }
    }
    draw_footer(frame, outer[1], state);
    if matches!(state.mode, InputMode::Picker) {
        draw_picker(frame, outer[0], state);
    }
}

fn draw_tree(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    frame.render_widget(Clear, area);
    let paragraph = Paragraph::new(Text::from(state.tree_lines.clone()))
        .style(Style::default().bg(Color::Indexed(236)))
        .wrap(Wrap { trim: false });
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
    let preview =
        Paragraph::new(state.preview_text.clone()).style(Style::default().bg(Color::Indexed(234)));

    frame.render_widget(status, sections[0]);
    frame.render_widget(preview, sections[1]);
}

fn draw_picker(frame: &mut Frame<'_>, area: Rect, state: &DrawState<'_>) {
    let area = centered_rect(area, 86, 80);
    frame.render_widget(Clear, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(sections[1]);
    let prompt = Paragraph::new(format!("picker: {}", current_input(state)))
        .style(Style::default().bg(Color::Indexed(236)).fg(Color::White))
        .wrap(Wrap { trim: false });
    let results = Paragraph::new(Text::from(state.picker_lines.clone()))
        .style(Style::default().bg(Color::Indexed(236)))
        .wrap(Wrap { trim: false });
    let preview = Paragraph::new(state.picker_preview.clone())
        .style(Style::default().bg(Color::Indexed(234)));

    frame.render_widget(prompt, sections[0]);
    frame.render_widget(results, body[0]);
    frame.render_widget(preview, body[1]);
}

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
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
            spans.push(Span::raw(" "));
        }
        spans.push(Span::raw(message));
    }
    let paragraph = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(Color::Indexed(236)))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn current_input(state: &DrawState<'_>) -> String {
    match state.mode {
        InputMode::Command => state.input.to_owned(),
        InputMode::Prompt(_) => state.input.to_owned(),
        InputMode::Filter => state.filter.to_owned(),
        InputMode::Search => state.input.to_owned(),
        InputMode::Picker => state.input.to_owned(),
        _ => String::new(),
    }
}

fn command_message(state: &DrawState<'_>) -> Option<String> {
    match state.mode {
        InputMode::Normal => None,
        InputMode::Command => Some(format!(":{}", current_input(state))),
        InputMode::Filter if state.show_hints => Some(format!("filter: {}", current_input(state))),
        InputMode::Filter => Some(format!("f{}", current_input(state))),
        InputMode::Search if state.show_hints => Some(format!("search: {}", current_input(state))),
        InputMode::Search => Some(format!("/{}", current_input(state))),
        InputMode::Picker => None,
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

fn pane_tree_label(pane_idx: usize) -> String {
    (pane_idx + 1).to_string()
}

fn pane_tree_line(window: &crate::tmux::Window, pane_idx: usize, zoom_suffix: &str) -> String {
    if window.panes.len() > 1 {
        format!(
            "{}{}{}",
            " ".repeat(window.name.chars().count() + 3),
            pane_tree_label(pane_idx),
            zoom_suffix
        )
    } else {
        format!("    {}{}", pane_tree_label(pane_idx), zoom_suffix)
    }
}

fn marker_column(line: String, pinned: bool, caffeinated: bool) -> String {
    let mut marker = String::new();
    if caffeinated {
        marker.push('☼');
    }
    if pinned {
        marker.push('⚲');
    }

    if marker.is_empty() {
        line
    } else {
        replace_leading_spaces(&line, &marker)
    }
}

fn replace_leading_spaces(line: &str, marker: &str) -> String {
    let leading_spaces = line.chars().take_while(|ch| *ch == ' ').count();
    if leading_spaces >= marker.chars().count() {
        let rest = line
            .chars()
            .skip(marker.chars().count())
            .collect::<String>();
        format!("{marker}{rest}")
    } else if leading_spaces > 0 {
        let rest = line.chars().skip(leading_spaces).collect::<String>();
        format!("{marker}{rest}")
    } else {
        format!("{marker} {line}")
    }
}

fn session_label(name: &str, caffeinated: bool) -> String {
    if caffeinated {
        format!("{name} ☼")
    } else {
        name.to_owned()
    }
}

fn window_tree_label(window: &crate::tmux::Window) -> String {
    if window.panes.len() > 1 {
        format!("{} 1", window.name)
    } else {
        window.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{marker_column, pane_tree_label, pane_tree_line, session_label, window_tree_label};
    use crate::{
        app::App,
        managed_config::ManagedConfig,
        tmux::{Pane, Session, Snapshot, Tmux, Window},
    };
    use std::collections::HashSet;

    #[test]
    fn pane_tree_label_uses_window_local_numbers() {
        assert_eq!(pane_tree_label(0), "1");
        assert_eq!(pane_tree_label(1), "2");
    }

    #[test]
    fn pane_tree_line_aligns_under_split_window_suffix() {
        let split_window = Window {
            id: String::from("@1"),
            name: String::from("zsh"),
            active: true,
            session_id: String::from("$1"),
            panes: vec![
                crate::tmux::Pane {
                    id: String::from("%1"),
                    current_command: String::from("zsh"),
                    current_path: String::from("/tmp"),
                    active: true,
                    zoomed: false,
                    window_id: String::from("@1"),
                },
                crate::tmux::Pane {
                    id: String::from("%2"),
                    current_command: String::from("zsh"),
                    current_path: String::from("/tmp"),
                    active: false,
                    zoomed: false,
                    window_id: String::from("@1"),
                },
            ],
        };

        assert_eq!(window_tree_label(&split_window), "zsh 1");
        assert_eq!(pane_tree_line(&split_window, 1, ""), "      2");
    }

    #[test]
    fn window_tree_label_suffixes_first_pane_when_split() {
        let split_window = Window {
            id: String::from("@1"),
            name: String::from("editor"),
            active: true,
            session_id: String::from("$1"),
            panes: vec![
                crate::tmux::Pane {
                    id: String::from("%1"),
                    current_command: String::from("zsh"),
                    current_path: String::from("/tmp"),
                    active: true,
                    zoomed: false,
                    window_id: String::from("@1"),
                },
                crate::tmux::Pane {
                    id: String::from("%2"),
                    current_command: String::from("zsh"),
                    current_path: String::from("/tmp"),
                    active: false,
                    zoomed: false,
                    window_id: String::from("@1"),
                },
            ],
        };

        assert_eq!(window_tree_label(&split_window), "editor 1");
    }

    #[test]
    fn marker_column_marks_rows_without_shifting_labels() {
        assert_eq!(
            marker_column(String::from("  editor 1"), true, false),
            "⚲ editor 1"
        );
        assert_eq!(
            marker_column(String::from("  editor 1"), false, true),
            "☼ editor 1"
        );
        assert_eq!(
            marker_column(String::from("  editor 1"), true, true),
            "☼⚲editor 1"
        );
        assert_eq!(
            marker_column(String::from("      2"), false, false),
            "      2"
        );
    }

    #[test]
    fn session_label_places_marker_after_name() {
        assert_eq!(session_label("node", false), "node");
        assert_eq!(session_label("node", true), "node ☼");
    }

    #[test]
    fn auto_sidebar_width_fits_visible_tree_labels() {
        let managed = ManagedConfig::bootstrap().expect("config");
        let mut app = App::new(Tmux::new(managed));
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$1"),
                name: String::from("long-session"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@1"),
                    name: String::from("build"),
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

        let state = super::DrawState::from_app(&app);

        assert_eq!(state.auto_sidebar_width(120), 13);
        assert_eq!(state.auto_sidebar_width(24), 8);
    }

    #[test]
    fn draw_state_places_session_marker_after_name() {
        let managed = ManagedConfig::bootstrap().expect("config");
        let mut app = App::new(Tmux::new(managed));
        app.snapshot = Snapshot {
            sessions: vec![Session {
                id: String::from("$4"),
                name: String::from("4"),
                attached: false,
                windows: vec![Window {
                    id: String::from("@8"),
                    name: String::from("zsh"),
                    active: true,
                    session_id: String::from("$4"),
                    panes: vec![Pane {
                        id: String::from("%9"),
                        current_command: String::from("zsh"),
                        current_path: String::from("/tmp"),
                        active: true,
                        zoomed: false,
                        window_id: String::from("@8"),
                    }],
                }],
            }],
        };
        app.caffeinated_targets = HashSet::from([String::from("$4")]);

        let state = super::DrawState::from_app(&app);
        let rendered = state.tree_lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered, "4 ☼");
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
