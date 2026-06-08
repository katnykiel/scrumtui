use chrono::Local;
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
use crate::ui::popup::render_burnup_chart;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    // Split horizontally: left = sprint list (~28 cols), right = detail + chart
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(10)])
        .split(area);

    render_sprint_list(f, app, chunks[0]);
    render_sprint_detail(f, app, chunks[1]);
}

fn render_sprint_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .history_sprints
        .iter()
        .enumerate()
        .map(|(i, sprint)| {
            let is_sel = i == app.history_sel;
            let pointer = if is_sel { "▶" } else { " " };
            let active_marker = if sprint.is_active { " ●" } else { "  " };
            let name_w = (area.width as usize).saturating_sub(8);
            let name = trunc(&sprint.name, name_w);
            let date_range = format!(
                "  {} → {}",
                sprint.start_date.format("%b %d"),
                sprint.end_date.format("%b %d, %Y"),
            );

            let name_style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if sprint.is_active {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{pointer}{active_marker} "), Style::default().fg(Color::Magenta)),
                    Span::styled(name, name_style),
                ]),
                Line::from(Span::styled(date_range, Style::default().fg(Color::DarkGray))),
            ])
        })
        .collect();

    let empty_hint = if app.history_sprints.is_empty() {
        "  No sprints yet."
    } else {
        ""
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        "sprints",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                ]))
                .title_bottom(Line::from(Span::styled(
                    " [j/k] navigate ",
                    Style::default().fg(Color::DarkGray),
                )))
                .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));

    if app.history_sprints.is_empty() {
        f.render_widget(
            Paragraph::new(empty_hint)
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "sprints",
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(" "),
                        ]))
                        .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
                ),
            area,
        );
    } else {
        let mut state = ratatui::widgets::ListState::default();
        state.select(Some(app.history_sel));
        f.render_stateful_widget(list, area, &mut state);
    }
}

fn render_sprint_detail(f: &mut Frame, app: &App, area: Rect) {
    let sprint = match app.history_sprints.get(app.history_sel) {
        Some(s) => s,
        None => {
            f.render_widget(
                Paragraph::new("  No sprint selected.")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(Color::Rgb(60, 60, 90))),
                    ),
                area,
            );
            return;
        }
    };

    // Split right pane: left = stats + issue list, right = burnup chart
    // Chart gets ~40% of space, minimum 36 cols to be useful
    let right_chart_w = ((area.width as usize) * 2 / 5).max(36).min(area.width as usize - 20) as u16;
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(right_chart_w)])
        .split(area);

    let issues = &app.history_issues;

    // ── Left: stats header + issue list ───────────────────────────────────────
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(1)])
        .split(h_chunks[0]);

    render_stats_header(f, app, left_chunks[0], sprint, issues);
    render_issue_list(f, issues, left_chunks[1]);

    // ── Right: burnup chart ───────────────────────────────────────────────────
    let sprint_issue_refs: Vec<&crate::models::Issue> = issues.iter().collect();
    render_burnup_chart(f, sprint.start_date, sprint.end_date, &sprint_issue_refs, h_chunks[1]);
}

fn render_stats_header(
    f: &mut Frame,
    _app: &App,
    area: Rect,
    sprint: &crate::models::Sprint,
    issues: &[crate::models::Issue],
) {
    let total_issues: usize = issues.len();
    let done_issues = issues.iter().filter(|i| i.status == Status::Done).count();
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    let done_sp: f64 = issues.iter().filter(|i| i.status == Status::Done).map(|i| i.story_points).sum();
    let today = Local::now().date_naive();
    let duration_days = (sprint.end_date - sprint.start_date).num_days() + 1;
    let elapsed_days = (today - sprint.start_date).num_days().clamp(0, duration_days);
    let status_label = if sprint.is_active {
        Span::styled("  ACTIVE", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else if today < sprint.start_date {
        Span::styled("  UPCOMING", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("  COMPLETED", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))
    };

    let header_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("  {} ", sprint.name),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            status_label,
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!(
                    "{} → {}  ({} days)",
                    sprint.start_date.format("%Y-%m-%d"),
                    sprint.end_date.format("%Y-%m-%d"),
                    duration_days,
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {done_issues}/{total_issues} issues done"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("   {}/{}sp completed", format_sp(done_sp), format_sp(total_sp)),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  Day {elapsed_days}/{duration_days}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    f.render_widget(
        Paragraph::new(header_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        "sprint history",
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                ]))
                .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
        ),
        area,
    );
}

fn render_issue_list(f: &mut Frame, issues: &[crate::models::Issue], area: Rect) {
    let col_w = area.width as usize;
    let title_max = col_w.saturating_sub(30);

    let list_items: Vec<ListItem> = issues
        .iter()
        .map(|issue| {
            let sym = status_symbol(&issue.status);
            let sc = status_color(&issue.status);
            let due = issue
                .due_date
                .map(|d| format!("  {}", d.format("%b %d")))
                .unwrap_or_default();
            Line::from(vec![
                Span::styled(format!("  {sym} "), Style::default().fg(sc)),
                Span::styled(
                    format!("{:<width$}", trunc(&issue.title, title_max), width = title_max),
                    if issue.status == Status::Done {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    format!("  #{}", issue.id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  {:>4}sp", format_sp(issue.story_points)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    format!("  {:<4}", issue.status.short()),
                    Style::default().fg(sc),
                ),
                Span::styled(
                    format!("  {:<12}", trunc(&issue.epic, 12)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(due, Style::default().fg(Color::DarkGray)),
            ])
            .into()
        })
        .map(|line: Line| ListItem::new(line))
        .collect();

    let empty_msg = if issues.is_empty() {
        "  No issues in this sprint."
    } else {
        ""
    };

    if issues.is_empty() {
        f.render_widget(
            Paragraph::new(empty_msg)
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(Line::from(Span::styled(
                            " issues ",
                            Style::default().fg(Color::DarkGray),
                        )))
                        .border_style(Style::default().fg(Color::Rgb(60, 60, 90))),
                ),
            area,
        );
    } else {
        f.render_widget(
            List::new(list_items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(Line::from(Span::styled(
                        " issues ",
                        Style::default().fg(Color::DarkGray),
                    )))
                    .title_bottom(Line::from(Span::styled(
                        " [1]backlog  [2]kanban  [3]gantt  [4]history  [?]help ",
                        Style::default().fg(Color::DarkGray),
                    )))
                    .border_style(Style::default().fg(Color::Rgb(60, 60, 90))),
            ),
            area,
        );
    }
}
