use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};

const STATUSES: [Status; 3] = [Status::Todo, Status::InProgress, Status::Done];

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(area);

    for (col_idx, status) in STATUSES.iter().enumerate() {
        render_column(f, app, cols[col_idx], col_idx, status);
    }
}

fn render_column(f: &mut Frame, app: &mut App, area: Rect, col_idx: usize, status: &Status) {
    let issues = app.sprint_issues_by_status(status);
    let is_active_col = app.kanban_col == col_idx;
    let selected_row = app.kanban_rows[col_idx];
    let sc = status_color(status);
    let sym = status_symbol(status);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(2)])
        .split(area);

    // Header
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    let header_style = if is_active_col {
        Style::default().fg(sc).add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(sc).add_modifier(Modifier::BOLD)
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {sym} "), Style::default().fg(sc)),
        Span::styled(status.label(), header_style),
        Span::styled(
            format!("  {} issues · {}sp ", issues.len(), format_sp(total_sp)),
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if is_active_col {
                Style::default().fg(sc)
            } else {
                Style::default().fg(Color::Rgb(60, 60, 80))
            }),
    );
    f.render_widget(header, chunks[0]);

    // Issue list
    let col_width = area.width as usize;
    let items: Vec<ListItem> = issues
        .iter()
        .enumerate()
        .map(|(row, issue)| {
            let is_sel = is_active_col && row == selected_row;
            let pointer = if is_sel { "▶" } else { " " };
            let due = issue
                .due_date
                .map(|d| format!("  {}", d.format("%b %d")))
                .unwrap_or_default();

            if issue.is_subtask() {
                // Subtask card: show parent label + title
                let parent_label = issue.parent_id
                    .and_then(|pid| app.issue_by_id(pid))
                    .map(|p| trunc(&p.title, col_width.saturating_sub(6)))
                    .unwrap_or_default();
                let lines = vec![
                    Line::from(vec![
                        Span::styled(pointer, Style::default().fg(Color::Magenta)),
                        Span::styled(
                            format!(" {}", trunc(&issue.title, col_width.saturating_sub(3))),
                            Style::default(),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("   ↳ {}{}", parent_label, due),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                ];
                ListItem::new(lines)
            } else {
                // Regular issue card
                let (done_subs, total_subs) = app.subtask_counts(issue.id);
                let subtask_badge = if total_subs > 0 {
                    format!("  [{}/{}]", done_subs, total_subs)
                } else {
                    String::new()
                };
                let auto_status = if total_subs > 0 {
                    format!("  ⊙")
                } else {
                    String::new()
                };
                let lines = vec![
                    Line::from(vec![
                        Span::styled(pointer, Style::default().fg(Color::Magenta)),
                        Span::styled(
                            format!(" {}", trunc(&issue.title, col_width.saturating_sub(3))),
                            Style::default(),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("   {}sp  {}{}{}{}", format_sp(issue.story_points), issue.epic, due, subtask_badge, auto_status),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                ];
                ListItem::new(lines)
            }
        })
        .collect();

    let mut list_state = ratatui::widgets::ListState::default();
    if is_active_col && !issues.is_empty() {
        list_state.select(Some(selected_row));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                .border_type(BorderType::Rounded)
                .border_style(if is_active_col {
                    Style::default().fg(sc)
                } else {
                    Style::default().fg(Color::Rgb(60, 60, 80))
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));

    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Hint bar
    let hint = if is_active_col {
        Line::from(Span::styled(
            " [>] advance  [<] regress  [e] edit  [h/l] switch col ",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(Span::styled(
            " [h/l] switch col",
            Style::default().fg(Color::DarkGray),
        ))
    };
    f.render_widget(
        Paragraph::new(hint).block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_type(BorderType::Rounded)
                .border_style(if is_active_col {
                    Style::default().fg(sc)
                } else {
                    Style::default().fg(Color::Rgb(60, 60, 80))
                }),
        ),
        chunks[2],
    );
}
