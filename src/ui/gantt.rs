use chrono::{Duration, Local, NaiveDate};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::app::{epic_detail_filter, App, Popup};
use crate::models::{format_sp, Issue, Status};
use crate::ui::backlog::{status_color, status_symbol, trunc};
use crate::ui::{hint, BORDER, BORDER_DIM};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title_bottom(hint("[j/k] epic  [e] issues of epic  [g/G] top/bottom  [?] help"))
        .border_style(Style::default().fg(BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.issues.is_empty() {
        f.render_widget(
            Paragraph::new("  No issues yet. Go to the backlog (1) and press n to create one.")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let today = Local::now().date_naive();

    // Only top-level issues
    let issues: Vec<&Issue> = app.issues.iter().filter(|i| i.parent_id.is_none()).collect();

    let bar_end_of = |i: &Issue| -> NaiveDate {
        if i.status == Status::Done {
            i.completed_at.map(|dt| dt.date()).or(i.due_date).unwrap_or(today)
        } else {
            i.due_date.unwrap_or(today).max(today)
        }
    };

    // Global x-axis: shared across all epics, always including today.
    let timeline_start = issues.iter()
        .map(|i| i.created_at.date())
        .min()
        .unwrap_or(today)
        .min(today);
    let timeline_end = issues.iter()
        .map(|i| bar_end_of(i))
        .max()
        .unwrap_or(today)
        .max(timeline_start + Duration::days(7));
    let total_days = (timeline_end - timeline_start).num_days().max(1) as usize + 1;

    // Columns: "▶ ◆ " + name + "  [" + bar + "]" + counts.
    let epics = app.epics().to_vec();
    let inner_w = inner.width as usize;
    let name_w = epics.iter().map(|e| e.chars().count()).max().unwrap_or(8).clamp(8, 24);
    const COUNTS_W: usize = 24;
    let bar_w = inner_w.saturating_sub(4 + name_w + 3 + 1 + COUNTS_W).max(10);
    let col_of = |d: NaiveDate| -> usize {
        ((d - timeline_start).num_days().max(0) as usize * bar_w / total_days).min(bar_w - 1)
    };
    let today_col = col_of(today);

    // ── Header: a date label every week, aligned with the bars ────────────────
    let lead = " ".repeat(4 + name_w + 3);
    let mut labels: Vec<char> = vec![' '; bar_w];
    let mut d = timeline_start;
    let mut last_end = 0usize;
    while d <= timeline_end {
        let col = col_of(d);
        let text = d.format("%b %d").to_string();
        if col >= last_end && col + text.len() <= bar_w {
            for (i, ch) in text.chars().enumerate() {
                labels[col + i] = ch;
            }
            last_end = col + text.len() + 1;
        }
        d += Duration::days(7);
    }
    let header = Line::from(vec![
        Span::raw(lead.clone()),
        Span::styled(labels.into_iter().collect::<String>(), Style::default().fg(Color::DarkGray)),
    ]);
    // "▾ today" under today's column (or "today ▾" when it's near the right edge).
    let today_line = if today_col + 7 <= bar_w {
        Line::from(vec![
            Span::raw(format!("{lead}{}", " ".repeat(today_col))),
            Span::styled("▾", Style::default().fg(Color::Magenta)),
            Span::styled(" today", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::raw(format!("{lead}{}", " ".repeat(today_col.saturating_sub(6)))),
            Span::styled("today ", Style::default().fg(Color::DarkGray)),
            Span::styled("▾", Style::default().fg(Color::Magenta)),
        ])
    };

    // ── Epic rows ───────────────────────────────────────────────────────────
    let mut rows: Vec<Line> = Vec::new();
    for (epic_idx, epic) in epics.iter().enumerate() {
        let epic_issues: Vec<&&Issue> = issues.iter().filter(|i| &i.epic == epic).collect();
        if epic_issues.is_empty() {
            continue;
        }

        // Epic bar: spans from earliest created_at to latest bar_end
        let epic_start = epic_issues.iter().map(|i| i.created_at.date()).min().unwrap_or(timeline_start);
        let epic_end = epic_issues.iter().map(|i| bar_end_of(i)).max().unwrap_or(today);

        let todo_count = epic_issues.iter().filter(|i| i.status == Status::Todo).count();
        let ip_count = epic_issues.iter().filter(|i| i.status == Status::InProgress).count();
        let done_count = epic_issues.iter().filter(|i| i.status == Status::Done).count();

        // Epic color mirrors the subtask roll-up: all done → green, all todo → yellow,
        // any mix → cyan.
        let (color, fill) = if done_count == epic_issues.len() {
            (Color::Green, '█')
        } else if todo_count == epic_issues.len() {
            (Color::Yellow, '░')
        } else {
            (Color::Cyan, '▓')
        };

        let (from, to) = (col_of(epic_start), col_of(epic_end).max(col_of(epic_start)));
        let bar: String = (0..bar_w)
            .map(|c| {
                if c >= from && c <= to {
                    fill
                } else if c == today_col {
                    '┊'
                } else {
                    ' '
                }
            })
            .collect();

        let total_sp: f64 = epic_issues.iter().map(|i| i.story_points).sum();
        let done_sp: f64 = epic_issues.iter()
            .filter(|i| i.status == Status::Done)
            .map(|i| i.story_points)
            .sum();
        let counts = format!(
            " {done_count}✓ {ip_count}◉ {todo_count}○  {}/{}sp",
            format_sp(done_sp),
            format_sp(total_sp),
        );

        let is_sel = epic_idx == app.gantt_sel;
        let name_style = if is_sel {
            Style::default().fg(color).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(color).add_modifier(Modifier::BOLD)
        };
        rows.push(Line::from(vec![
            Span::styled(if is_sel { "▶ ◆ " } else { "  ◆ " }, Style::default().fg(Color::Magenta)),
            Span::styled(format!("{:<name_w$}", trunc(epic, name_w)), name_style),
            Span::styled("  [", Style::default().fg(Color::DarkGray)),
            Span::styled(bar, Style::default().fg(color)),
            Span::styled("]", Style::default().fg(Color::DarkGray)),
            Span::styled(counts, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Keep the selected epic on screen: scroll only when it would leave the view.
    let visible = inner.height.saturating_sub(3) as usize;
    let sel = app.gantt_sel.min(rows.len().saturating_sub(1));
    if sel < app.gantt_scroll {
        app.gantt_scroll = sel;
    } else if visible > 0 && sel >= app.gantt_scroll + visible {
        app.gantt_scroll = sel + 1 - visible;
    }

    let mut lines = vec![
        header,
        today_line,
        Line::from(Span::styled("─".repeat(inner_w), Style::default().fg(Color::DarkGray))),
    ];
    lines.extend(rows.into_iter().skip(app.gantt_scroll));
    f.render_widget(Paragraph::new(lines), inner);
}

// ── Epic detail popup ──────────────────────────────────────────────────────────

pub fn render_epic_detail_popup(f: &mut Frame, popup: &Popup) {
    let (epic, issues, search, search_active, sel) = match popup {
        Popup::GanttEpicDetail { epic, issues, search, search_active, sel, .. } => {
            (epic, issues, search, *search_active, *sel)
        }
        _ => return,
    };

    let area = f.area();
    // Never larger than the screen: drawing outside the buffer panics.
    let width = area.width.min(96);
    let height = area.height.min(40);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup_area = Rect { x, y, width, height };

    f.render_widget(Clear, popup_area);

    let filtered = epic_detail_filter(issues, search);

    let count = |s: Status| issues.iter().filter(|i| i.status == s).count();
    let sp = |s: Status| -> f64 { issues.iter().filter(|i| i.status == s).map(|i| i.story_points).sum() };
    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();

    let bottom_hint = if search_active {
        "[Enter] keep filter  [Esc] clear"
    } else {
        "[j/k] move  [e] edit  [/] search  [Esc] close"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::styled(" ◆ ", Style::default().fg(Color::Magenta)),
            Span::styled(epic.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .title_bottom(hint(bottom_hint))
        .border_style(Style::default().fg(BORDER_DIM));

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    // Split: summary row (2 lines) + optional search (1) + issue list (fill)
    let show_search = search_active || !search.is_empty();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(if show_search { 1 } else { 0 }),
            Constraint::Min(1),
        ])
        .split(inner);

    // ── Summary ─────────────────────────────────────────────────────────────
    let summary = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  ◉ {} in progress", count(Status::InProgress)), Style::default().fg(Color::Cyan)),
            Span::styled(format!("   ○ {} to do", count(Status::Todo)), Style::default().fg(Color::Yellow)),
            Span::styled(format!("   ✓ {} done", count(Status::Done)), Style::default().fg(Color::Green)),
            Span::styled(
                format!(
                    "    {}/{}sp done ({}sp in progress)",
                    format_sp(sp(Status::Done)),
                    format_sp(total_sp),
                    format_sp(sp(Status::InProgress)),
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
    ];
    f.render_widget(Paragraph::new(summary), chunks[0]);

    // ── Search bar ─────────────────────────────────────────────────────────
    if show_search {
        let cursor = if search_active { "▌" } else { "" };
        let style = if search_active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("  / {search}{cursor}"), style))),
            chunks[1],
        );
    }

    // ── Issue list ─────────────────────────────────────────────────────────
    let list_chunk = chunks[2];
    if filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  No matching issues.", Style::default().fg(Color::DarkGray))),
            list_chunk,
        );
        return;
    }
    let title_max = (list_chunk.width as usize).saturating_sub(26);
    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(idx, issue)| {
            let is_sel = idx == sel;
            let sc = status_color(&issue.status);
            let is_done = issue.status == Status::Done;
            let title_style = match (is_sel, is_done) {
                (true, _) => Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                (false, true) => Style::default().fg(Color::DarkGray),
                (false, false) => Style::default(),
            };
            let due = issue.due_date.map(|d| format!("  {}", d.format("%b %d"))).unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(if is_sel { "▶ " } else { "  " }, Style::default().fg(Color::Magenta)),
                Span::styled(format!("{} ", status_symbol(&issue.status)), Style::default().fg(sc)),
                Span::styled(format!("{:<title_max$}", trunc(&issue.title, title_max)), title_style),
                Span::styled(format!(" {:>3}sp", format_sp(issue.story_points)), Style::default().fg(Color::Magenta)),
                Span::styled(format!("  {:<4}", issue.status.short()), Style::default().fg(sc)),
                Span::styled(due, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(sel));
    f.render_stateful_widget(List::new(items), list_chunk, &mut state);
}
