use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Sprint, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};
use crate::ui::popup::render_burnup_chart;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
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
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::REVERSED)
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

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("sprints", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                ]))
                .title_bottom(Line::from(Span::styled(
                    " [j/k/g/G] navigate  [PgDn/PgUp] page  [e] rename  [d] delete ",
                    Style::default().fg(Color::DarkGray),
                )))
                .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));

    if app.history_sprints.is_empty() {
        f.render_widget(
            Paragraph::new("  No sprints yet.")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(Line::from(vec![
                            Span::raw(" "),
                            Span::styled("sprints", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
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

    // Right column: burnup chart (top) + analysis panel (bottom, ~12 rows)
    let right_col_w = ((area.width as usize) * 2 / 5).max(38).min(area.width as usize - 20) as u16;
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(right_col_w)])
        .split(area);

    let issues = &app.history_issues;

    // Left: stats header (6 rows) + issue list (fill)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(1)])
        .split(h_chunks[0]);

    render_stats_header(f, left_chunks[0], sprint, issues);
    render_issue_list(f, issues, left_chunks[1]);

    // Right: burnup chart (top) + analysis panel (bottom)
    let analysis_h = 13u16.min(h_chunks[1].height.saturating_sub(8));
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(analysis_h)])
        .split(h_chunks[1]);

    let sprint_issue_refs: Vec<&crate::models::Issue> = issues.iter().collect();
    render_burnup_chart(f, sprint.start_date, sprint.end_date, &sprint_issue_refs, right_chunks[0]);
    render_analysis_panel(f, app, right_chunks[1], sprint, issues);
}

/// Effective sprint duration in days.
/// Sprints recorded with start == end (1 day) are treated as 7-day sprints,
/// since they were almost certainly created without setting a real end date.
fn effective_duration(sprint: &Sprint) -> i64 {
    let raw = (sprint.end_date - sprint.start_date).num_days() + 1;
    if raw <= 1 { 7 } else { raw }
}

fn render_stats_header(
    f: &mut Frame,
    area: Rect,
    sprint: &Sprint,
    issues: &[crate::models::Issue],
) {
    let total_issues = issues.len();
    let done_issues = issues.iter().filter(|i| i.status == Status::Done).count();
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    let done_sp: f64 = issues.iter().filter(|i| i.status == Status::Done).map(|i| i.story_points).sum();
    let today = Local::now().date_naive();
    let duration_days = effective_duration(sprint);
    let elapsed_days = (today - sprint.start_date).num_days().clamp(0, duration_days);

    let status_label = if sprint.is_active {
        Span::styled("  ACTIVE", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else if today < sprint.start_date {
        Span::styled("  UPCOMING", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("  COMPLETED", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD))
    };

    let header_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {} ", sprint.name), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            status_label,
        ]),
        Line::from(Span::styled(
            format!(
                "  {} → {}  ({} days)",
                sprint.start_date.format("%Y-%m-%d"),
                sprint.end_date.format("%Y-%m-%d"),
                duration_days,
            ),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(vec![
            Span::styled(format!("  {done_issues}/{total_issues} issues done"), Style::default().fg(Color::Gray)),
            Span::styled(
                format!("   {}/{}sp done", format_sp(done_sp), format_sp(total_sp)),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        if sprint.is_active {
            Line::from(Span::styled(
                format!("  Day {elapsed_days}/{duration_days}"),
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from("")
        },
    ];

    f.render_widget(
        Paragraph::new(header_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("sprint history", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                ]))
                .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
        ),
        area,
    );
}

fn render_issue_list(f: &mut Frame, issues: &[crate::models::Issue], area: Rect) {
    let col_w = area.width as usize;
    // Calculate column widths dynamically
    // sym(3) + title(flexible) + id(8) + sp(7) + status(6) + epic(12) + due(10)
    // Allocate roughly: title takes 40%, epic takes 25%, rest fixed
    let fixed_width = 3 + 8 + 7 + 6 + 10 + 4; // sym, id, sp, status, due, spacing
    let flexible = col_w.saturating_sub(fixed_width);
    let title_max = (flexible * 40 / 100).max(20);
    let epic_max = (flexible * 25 / 100).max(10);

    let list_items: Vec<ListItem> = issues
        .iter()
        .map(|issue| {
            let sym = status_symbol(&issue.status);
            let sc = status_color(&issue.status);
            let due = issue.due_date.map(|d| format!("  {}", d.format("%b %d"))).unwrap_or_default();
            Line::from(vec![
                Span::styled(format!("  {sym} "), Style::default().fg(sc)),
                Span::styled(
                    format!("{:<width$}", trunc(&issue.title, title_max), width = title_max),
                    if issue.status == Status::Done { Style::default().fg(Color::DarkGray) } else { Style::default() },
                ),
                Span::styled(format!("  #{}", issue.id), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("  {:>4}sp", format_sp(issue.story_points)), Style::default().fg(Color::Magenta)),
                Span::styled(format!("  {:<4}", issue.status.short()), Style::default().fg(sc)),
                Span::styled(format!("  {:<width$}", trunc(&issue.epic, epic_max), width = epic_max), Style::default().fg(Color::Cyan)),
                Span::styled(due, Style::default().fg(Color::DarkGray)),
            ]).into()
        })
        .map(|line: Line| ListItem::new(line))
        .collect();

    let hint = " [1]backlog  [2]kanban  [3]gantt  [4]history  [?]help ";
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(Span::styled(" issues ", Style::default().fg(Color::DarkGray))))
        .title_bottom(Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))))
        .border_style(Style::default().fg(Color::Rgb(60, 60, 90)));

    if issues.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  No issues in this sprint.", Style::default().fg(Color::DarkGray)))
                .block(block),
            area,
        );
    } else {
        f.render_widget(List::new(list_items).block(block), area);
    }
}

/// Per-sprint done SP aligned to `history_sprints` order (newest first).
fn done_sp_series(app: &App) -> Vec<f64> {
    app.history_sprints
        .iter()
        .map(|s| {
            app.history_sprint_stats
                .iter()
                .find(|(id, _, _)| *id == s.id)
                .map(|(_, _, done)| *done)
                .unwrap_or(0.0)
        })
        .collect()
}

fn render_analysis_panel(
    f: &mut Frame,
    app: &App,
    area: Rect,
    sprint: &Sprint,
    issues: &[crate::models::Issue],
) {
    if area.height < 4 {
        return;
    }

    let series = done_sp_series(app);

    // ── Velocity per day: sprints completed BEFORE this sprint's end date ─────
    // Collect (done_sp, duration_days) pairs; newest-first order.
    let prior_data: Vec<(f64, i64)> = app
        .history_sprints
        .iter()
        .zip(series.iter())
        .filter(|(s, _)| !s.is_active && s.end_date < sprint.end_date)
        .map(|(s, sp)| (*sp, effective_duration(s)))
        .collect();

    let window = prior_data.len().min(5);

    // sp/day for each prior sprint
    let prior_spd: Vec<f64> = prior_data.iter()
        .map(|(sp, days)| if *days > 0 { sp / *days as f64 } else { 0.0 })
        .collect();

    let velocity_spd: Option<f64> = if window == 0 {
        None
    } else {
        Some(prior_spd[..window].iter().sum::<f64>() / window as f64)
    };

    // Trend over prior sprints: newest 2 sp/day vs the 2 before that
    let trend = if prior_spd.len() >= 4 {
        let r = (prior_spd[0] + prior_spd[1]) / 2.0;
        let o = (prior_spd[2] + prior_spd[3]) / 2.0;
        if r > o * 1.05 { ("↑", Color::Green) } else if r < o * 0.95 { ("↓", Color::Red) } else { ("→", Color::DarkGray) }
    } else {
        ("–", Color::DarkGray)
    };

    // ── This sprint ───────────────────────────────────────────────────────────
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    let done_sp: f64 = issues.iter().filter(|i| i.status == Status::Done).map(|i| i.story_points).sum();
    let pct = if total_sp > 0.0 { done_sp / total_sp * 100.0 } else { 0.0 };
    let sprint_days = effective_duration(sprint);

    // SP added mid-sprint: issues created strictly after sprint.start_date
    let mid_sp: f64 = issues
        .iter()
        .filter(|i| i.created_at.date() > sprint.start_date)
        .map(|i| i.story_points)
        .sum();

    // ── Recommended starting SP ───────────────────────────────────────────────
    // p25 of prior done-sp-per-day * this sprint's duration → reliable SP target.
    let safe_target: Option<f64> = if prior_spd.len() >= 3 {
        let mut sorted_spd = prior_spd[..prior_spd.len().min(10)].to_vec();
        sorted_spd.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p25_idx = (sorted_spd.len() as f64 * 0.25) as usize;
        let p25_spd = sorted_spd[p25_idx].min(velocity_spd.unwrap_or(f64::MAX));
        Some(p25_spd * sprint_days as f64)
    } else {
        velocity_spd.map(|v| v * sprint_days as f64)
    };

    // ── Build lines ───────────────────────────────────────────────────────────
    let mut lines: Vec<Line> = vec![Line::from("")];

    // Velocity as sp/day
    let vel_str = velocity_spd
        .map(|v| format!("{v:.2}sp/day"))
        .unwrap_or_else(|| "n/a".into());
    let no_data_label = if window == 0 { "  no prior data" } else { "" };
    lines.push(Line::from(vec![
        Span::styled("  velocity   ", Style::default().fg(Color::DarkGray)),
        Span::styled(vel_str, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(no_data_label, Style::default().fg(Color::DarkGray)),
        Span::styled(format!("  {}", trend.0), Style::default().fg(trend.1).add_modifier(Modifier::BOLD)),
    ]));

    // Completion rate
    let pct_color = if pct >= 80.0 { Color::Green } else if pct >= 50.0 { Color::Yellow } else { Color::Red };
    lines.push(Line::from(vec![
        Span::styled("  completed  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{pct:.0}%"), Style::default().fg(pct_color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("  ({}/{}sp)", format_sp(done_sp), format_sp(total_sp)), Style::default().fg(Color::DarkGray)),
    ]));

    // Safe start — just the number, color-coded green/yellow
    if let Some(target) = safe_target {
        let col = if total_sp <= target * 1.1 { Color::Green } else { Color::Yellow };
        lines.push(Line::from(vec![
            Span::styled("  safe start  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("≤{}sp", format_sp(target)), Style::default().fg(col).add_modifier(Modifier::BOLD)),
        ]));
    }

    // Scope creep — only show if something was added mid-sprint
    if total_sp > 0.0 && mid_sp > 0.0 {
        let creep_color = if mid_sp > total_sp * 0.2 { Color::Yellow } else { Color::DarkGray };
        lines.push(Line::from(vec![
            Span::styled("  scope       ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}sp", format_sp(total_sp - mid_sp)), Style::default().fg(Color::Gray)),
            Span::styled(format!("  +{}sp", format_sp(mid_sp)), Style::default().fg(creep_color)),
        ]));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("analysis", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                ]))
                .border_style(Style::default().fg(Color::Rgb(80, 60, 100))),
        ),
        area,
    );
}
