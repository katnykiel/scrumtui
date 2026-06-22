use chrono::{Duration, Local, NaiveDate};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{App, Popup};
use crate::models::{format_sp, Issue, Status};
use crate::ui::backlog::{status_color, trunc};

const BAR_WIDTH: usize = 40;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    if app.issues.is_empty() {
        f.render_widget(
            Paragraph::new("  No issues yet. Go to the backlog (1) and press n to create one.")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(Line::from(vec![
                            Span::raw(" "),
                            Span::styled(
                                "gantt",
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(" "),
                        ]))
                        .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
                ),
            area,
        );
        return;
    }

    let today = Local::now().date_naive();

    // Only top-level issues
    let issues: Vec<&Issue> = app.issues.iter().filter(|i| i.parent_id.is_none()).collect();

    let bar_end_of = |i: &&Issue| -> NaiveDate {
        if i.status == Status::Done {
            i.completed_at.map(|dt| dt.date()).or(i.due_date).unwrap_or(today)
        } else {
            i.due_date.unwrap_or(today)
        }
    };

    // Global x-axis: shared across all epics
    let timeline_start = issues.iter()
        .map(|i| i.created_at.date())
        .min()
        .unwrap_or(today - Duration::days(1));
    let timeline_end = issues.iter()
        .map(|i| bar_end_of(i))
        .max()
        .unwrap_or(today + Duration::days(14));
    let timeline_end = timeline_end.max(timeline_start + Duration::days(3));
    let total_days = (timeline_end - timeline_start).num_days().max(1) as usize;

    let inner_width = (area.width as usize).saturating_sub(2);

    let epics = app.epics();

    // ── Build lines ────────────────────────────────────────────────────────────
    let mut all_lines: Vec<(Line<'static>, bool)> = vec![]; // (line, is_epic_row)

    // Timeline header
    all_lines.push((build_timeline_header(timeline_start, timeline_end, inner_width), false));
    all_lines.push((Line::from(Span::styled(
        "─".repeat(inner_width),
        Style::default().fg(Color::DarkGray),
    )), false));

    for (epic_idx, epic) in epics.iter().enumerate() {
        let epic_issues: Vec<&&Issue> = issues.iter()
            .filter(|i| &i.epic == epic)
            .collect();
        if epic_issues.is_empty() {
            continue;
        }

        // Epic bar: spans from earliest created_at to latest bar_end
        let epic_start = epic_issues.iter()
            .map(|i| i.created_at.date())
            .min()
            .unwrap_or(timeline_start);
        let epic_end = epic_issues.iter()
            .map(|i| bar_end_of(i))
            .max()
            .unwrap_or(today);

        // Count statuses
        let todo_count = epic_issues.iter().filter(|i| i.status == Status::Todo).count();
        let ip_count   = epic_issues.iter().filter(|i| i.status == Status::InProgress).count();
        let done_count = epic_issues.iter().filter(|i| i.status == Status::Done).count();

        // Determine overall epic status color (mirrors subtask logic):
        // all done → green, all todo → yellow, any mix (todo+done or any IP) → cyan
        let (epic_status_color, epic_fill) = if done_count == epic_issues.len() {
            (Color::Green, '█')
        } else if todo_count == epic_issues.len() {
            (Color::Yellow, '░')
        } else {
            (Color::Cyan, '▓')
        };

        let bar = build_bar(epic_start, epic_end, timeline_start, total_days, epic_fill);
        let is_sel = epic_idx == app.gantt_sel;

        let pointer = if is_sel { "▶" } else { " " };
        let name_max = inner_width.saturating_sub(BAR_WIDTH + 20);
        let name_str = trunc(epic, name_max.max(8));

        // SP total
        let total_sp: f64 = epic_issues.iter().map(|i| i.story_points).sum();
        let done_sp: f64 = epic_issues.iter()
            .filter(|i| i.status == Status::Done)
            .map(|i| i.story_points)
            .sum();

        let counts_str = format!(
            "  {}✓ {}◉ {}○  {}/{}sp",
            done_count, ip_count, todo_count,
            format_sp(done_sp), format_sp(total_sp),
        );

        let epic_style = if is_sel {
            Style::default()
                .fg(epic_status_color)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(epic_status_color)
                .add_modifier(Modifier::BOLD)
        };

        all_lines.push((Line::from(vec![
            Span::styled(format!("{pointer} ◆ "), Style::default().fg(Color::Magenta)),
            Span::styled(format!("{:<width$}", name_str, width = name_max.max(8)), epic_style),
            Span::styled(
                format!("  {}", bar),
                Style::default().fg(epic_status_color),
            ),
            Span::styled(counts_str, Style::default().fg(Color::DarkGray)),
        ]), true));
    }

    let hint_str = " [e] detail  [/] search  [?] help ";

    // Skip lines for scroll
    let scroll = app.gantt_scroll.min(all_lines.len().saturating_sub(1));
    let visible: Vec<Line<'static>> = all_lines.into_iter().skip(scroll).map(|(l, _)| l).collect();

    let para = Paragraph::new(visible).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "gantt",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ]))
            .title_bottom(Line::from(Span::styled(
                hint_str,
                Style::default().fg(Color::DarkGray),
            )))
            .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
    );

    f.render_widget(para, area);
}

fn build_bar(
    start: NaiveDate,
    end: NaiveDate,
    timeline_start: NaiveDate,
    total_days: usize,
    fill_char: char,
) -> String {
    let bar_start_days = (start - timeline_start).num_days().max(0) as usize;
    let duration_days = (end - start).num_days().max(1) as usize;

    let bar_start_col = bar_start_days * BAR_WIDTH / total_days;
    let bar_len = (duration_days * BAR_WIDTH / total_days).max(1);
    let bar_end_col = (bar_start_col + bar_len).min(BAR_WIDTH);

    let mut bar = String::with_capacity(BAR_WIDTH + 2);
    bar.push('[');
    for i in 0..BAR_WIDTH {
        if i >= bar_start_col && i < bar_end_col {
            bar.push(fill_char);
        } else {
            bar.push(' ');
        }
    }
    bar.push(']');
    bar
}

fn build_timeline_header(
    start: NaiveDate,
    end: NaiveDate,
    inner_width: usize,
) -> Line<'static> {
    let total_days = (end - start).num_days().max(1) as usize;
    // Indent to match bar position (pointer + " ◆ " + name_max + "  ")
    // We use a fixed short label area for the header
    let lead = "   ";
    let mut header = format!("{lead}[");

    let mut bar_chars: Vec<char> = vec![' '; BAR_WIDTH];
    let mut d = start;
    while d <= end {
        let col = (d - start).num_days() as usize * BAR_WIDTH / total_days;
        let label = d.format("%m/%d").to_string();
        for (i, ch) in label.chars().enumerate() {
            let pos = col + i;
            if pos < BAR_WIDTH {
                bar_chars[pos] = ch;
            }
        }
        d = d + Duration::days(7);
    }
    header.extend(bar_chars.iter());
    header.push(']');

    if header.len() > inner_width {
        header.truncate(inner_width);
    }

    Line::from(Span::styled(header, Style::default().fg(Color::DarkGray)))
}

// ── Epic detail popup ──────────────────────────────────────────────────────────

pub fn render_epic_detail_popup(f: &mut Frame, popup: &Popup) {
    let (epic, issues, search, search_active, scroll) = match popup {
        Popup::GanttEpicDetail { epic, issues, search, search_active, scroll } => {
            (epic, issues, search, *search_active, *scroll)
        }
        _ => return,
    };

    let area = f.area();
    let width = (area.width.min(90)).max(40);
    let height = (area.height.min(40)).max(10);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup_area = Rect { x, y, width, height: height.min(area.height) };

    f.render_widget(Clear, popup_area);

    // Filter issues by search
    let q = search.to_lowercase();
    let filtered: Vec<&Issue> = issues.iter()
        .filter(|i| {
            if q.is_empty() { return true; }
            i.title.to_lowercase().contains(&q)
                || i.status.label().to_lowercase().contains(&q)
        })
        .collect();

    let todo_count  = issues.iter().filter(|i| i.status == Status::Todo).count();
    let ip_count    = issues.iter().filter(|i| i.status == Status::InProgress).count();
    let done_count  = issues.iter().filter(|i| i.status == Status::Done).count();
    let total_sp: f64  = issues.iter().map(|i| i.story_points).sum();
    let done_sp: f64   = issues.iter().filter(|i| i.status == Status::Done).map(|i| i.story_points).sum();
    let ip_sp: f64     = issues.iter().filter(|i| i.status == Status::InProgress).map(|i| i.story_points).sum();
    let todo_sp: f64   = issues.iter().filter(|i| i.status == Status::Todo).map(|i| i.story_points).sum();

    let bottom_hint = " [j/k] scroll  [/] search  [Esc/q] close ";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" ◆ "),
            Span::styled(
                epic.to_string(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            bottom_hint,
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Rgb(80, 80, 120)));

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Split: summary row (2 lines) + optional search (1) + issue list (fill)
    let show_search = search_active || !search.is_empty();
    let constraints = if show_search {
        vec![Constraint::Length(2), Constraint::Length(1), Constraint::Min(1)]
    } else {
        vec![Constraint::Length(2), Constraint::Min(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // ── Summary row ─────────────────────────────────────────────────────────
    let summary_lines = vec![
        Line::from(vec![
            Span::styled(
                format!("  ✓ {done_count} done  ◉ {ip_count} in progress  ○ {todo_count} todo"),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}sp done", format_sp(done_sp)),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!("  {}sp in progress", format_sp(ip_sp)),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("  {}sp todo", format_sp(todo_sp)),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!("  ({}sp total)", format_sp(total_sp)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(summary_lines), chunks[0]);

    let list_chunk = if show_search { chunks[2] } else { chunks[1] };

    // ── Search bar ─────────────────────────────────────────────────────────
    if show_search {
        let cursor = if search_active { "▌" } else { "" };
        let style = if search_active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  / {}{}", search, cursor),
                style,
            ))),
            chunks[1],
        );
    }

    // ── Issue list ─────────────────────────────────────────────────────────
    let col_w = list_chunk.width as usize;
    let title_max = col_w.saturating_sub(32);
    let items: Vec<ListItem> = filtered
        .iter()
        .skip(scroll)
        .map(|issue| {
            let sc = status_color(&issue.status);
            let sym = match issue.status {
                Status::Todo => "○",
                Status::InProgress => "◉",
                Status::Done => "✓",
            };
            let due = issue.due_date
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
                    format!("  {:>4}sp", format_sp(issue.story_points)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    format!("  {:<4}", issue.status.short()),
                    Style::default().fg(sc),
                ),
                Span::styled(due, Style::default().fg(Color::DarkGray)),
            ])
            .into()
        })
        .map(|l: Line| ListItem::new(l))
        .collect();

    if filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  No matching issues.",
                Style::default().fg(Color::DarkGray),
            )),
            list_chunk,
        );
    } else {
        f.render_widget(List::new(items), list_chunk);
    }
}
