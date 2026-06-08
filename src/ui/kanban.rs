use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};

/// Color for a due date relative to today.
/// - Overdue → Red
/// - Due within 7 days → Yellow
/// - Otherwise → DarkGray
fn due_date_color(due: &chrono::NaiveDate) -> Color {
    let today = Local::now().date_naive();
    let days = (*due - today).num_days();
    if days < 0 {
        Color::Red
    } else if days < 7 {
        Color::Yellow
    } else {
        Color::DarkGray
    }
}

const STATUSES: [Status; 3] = [Status::Todo, Status::InProgress, Status::Done];

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    // Outer titled box for visual consistency with other views
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "kanban",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            " [j/k] nav  [h/l] col  []/[] status  [e] edit  [?]help ",
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Rgb(80, 80, 120)));

    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Split inner vertically: top = kanban columns, bottom = detail pane (8 rows)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(8)])
        .split(inner);

    let col_area = chunks[0];
    let detail_area = chunks[1];

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(col_area);

    for (col_idx, status) in STATUSES.iter().enumerate() {
        render_column(f, app, cols[col_idx], col_idx, status);
    }

    render_detail(f, app, detail_area);
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
                Style::default().fg(Color::Rgb(60, 60, 90))
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

            // Due date with urgency color (skip for Done issues — already completed)
            let due_color = issue.due_date
                .map(|d| if issue.status == Status::Done { Color::DarkGray } else { due_date_color(&d) })
                .unwrap_or(Color::DarkGray);
            let due_str = issue
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
                            format!("   ↳ {}", parent_label),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(due_str, Style::default().fg(due_color)),
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
                let auto_status = if total_subs > 0 { "  ⊙" } else { "" };
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
                            format!("   #{} · {}sp  {}{}{}", issue.id, format_sp(issue.story_points), issue.epic, subtask_badge, auto_status),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(due_str, Style::default().fg(due_color)),
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
                    Style::default().fg(Color::Rgb(60, 60, 90))
                }),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));

    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Hint bar
    let hint = if is_active_col {
        Line::from(Span::styled(
            " [j/k] nav  []/[] status  [e] edit  [h/l] col ",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(Span::styled(
            " [h/l] switch col ",
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
                    Style::default().fg(Color::Rgb(60, 60, 90))
                }),
        ),
        chunks[2],
    );
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(60, 60, 90)))
        .title(Span::styled(" detail ", Style::default().fg(Color::DarkGray)));

    let issue = app.selected_kanban_issue();
    if issue.is_none() {
        f.render_widget(
            Paragraph::new("  No issue selected.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }
    let issue = issue.unwrap();

    let sc = status_color(&issue.status);
    let sym = status_symbol(&issue.status);
    let desc = issue.description.as_deref().unwrap_or("No description.");
    let due_str = issue
        .due_date
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "—".into());
    let created = issue.created_at.format("%Y-%m-%d %H:%M").to_string();
    let updated = issue.updated_at.format("%Y-%m-%d %H:%M").to_string();
    let completed = issue
        .completed_at
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".into());

    let lines = vec![
        Line::from(vec![
            Span::styled(format!("  {sym} "), Style::default().fg(sc)),
            Span::styled(
                issue.title.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "   #{} · {}sp · {} · {}",
                    issue.id,
                    format_sp(issue.story_points),
                    issue.status.label(),
                    issue.epic
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("         ", Style::default()),
            Span::styled(desc, Style::default().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("  due ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<12}", due_str), Style::default().fg(Color::Yellow)),
            Span::styled("  created ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<17}", created), Style::default().fg(Color::Gray)),
            Span::styled("  updated ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<17}", updated), Style::default().fg(Color::Gray)),
            Span::styled("  done ", Style::default().fg(Color::DarkGray)),
            Span::styled(completed, Style::default().fg(Color::Gray)),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(block),
        area,
    );
}
