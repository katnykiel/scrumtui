pub mod backlog;
pub mod gantt;
pub mod history;
pub mod kanban;
pub mod popup;
#[cfg(test)]
mod snapshot;

use chrono::Local;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, View};
use crate::models::{format_duration, format_sp, Issue, Status};
use crate::ui::backlog::{status_color, status_symbol};

/// Border of the main panel of a view.
pub const BORDER: Color = Color::Rgb(100, 100, 160);
/// Border of secondary panels (detail, charts, unfocused lists).
pub const BORDER_DIM: Color = Color::Rgb(80, 80, 120);
/// Marker of the frog (the issue to do first).
pub const FROG: Color = Color::LightRed;

pub fn render(f: &mut Frame, app: &mut App) {
    let full = f.area();

    // Top bar (views + sprint progress), the view, and a status line while a
    // message is showing.
    let msg = app.current_status().map(|s| s.to_string());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(if msg.is_some() { 1 } else { 0 }),
        ])
        .split(full);
    render_top_bar(f, app, chunks[0]);
    let main_area = chunks[1];

    match &app.view {
        View::Backlog => backlog::render(f, app, main_area),
        View::Kanban => kanban::render(f, app, main_area),
        View::Gantt => gantt::render(f, app, main_area),
        View::SprintHistory => history::render(f, app, main_area),
    }

    if let Some(text) = msg {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(text, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ])),
            chunks[2],
        );
    }

    // Overlay popup if one is active.
    // Take the popup out of app temporarily so we can borrow both without cloning.
    if let Some(p) = app.popup.take() {
        match &p {
            crate::app::Popup::GanttEpicDetail { .. } => {
                gantt::render_epic_detail_popup(f, &p);
            }
            crate::app::Popup::Help => popup::render_help(f, &mut app.help_scroll),
            _ => popup::render(f, &p, app),
        }
        app.popup = Some(p);
    }
}

/// One line: the views (current one highlighted) on the left, the active sprint's
/// progress on the right.
fn render_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let tabs = [
        (View::Backlog, "1 backlog"),
        (View::Kanban, "2 kanban"),
        (View::Gantt, "3 gantt"),
        (View::SprintHistory, "4 history"),
    ];
    let mut left = vec![Span::styled(
        " scrumtui",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )];
    for (view, label) in tabs {
        let style = if app.view == view {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        left.push(Span::styled(format!("   {label}"), style));
    }
    let used: usize = left.iter().map(|s| s.width()).sum();
    // Keep a clear gap between the tabs and the sprint summary.
    let room = (area.width as usize).saturating_sub(used + 6);
    f.render_widget(Paragraph::new(Line::from(left)), area);
    f.render_widget(
        Paragraph::new(Line::from(sprint_summary(app, room))).alignment(Alignment::Right),
        area,
    );
}

/// The active sprint at a glance ("Week 43   day 3/5   8/20sp ━━━━──── 40%"),
/// with less detail when it has to fit in fewer than `room` columns.
fn sprint_summary(app: &App, room: usize) -> Vec<Span<'static>> {
    let dim = Style::default().fg(Color::DarkGray);
    let first_fitting = |options: Vec<Vec<Span<'static>>>| -> Vec<Span<'static>> {
        options
            .into_iter()
            .find(|spans| spans.iter().map(|s| s.width()).sum::<usize>() <= room)
            .unwrap_or_default()
    };
    let Some(sprint) = &app.active_sprint else {
        return first_fitting(vec![
            vec![Span::styled("no active sprint   S starts one ", dim)],
            vec![Span::styled("S: new sprint ", dim)],
        ]);
    };
    let name = Span::styled(
        sprint.name.clone(),
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    );
    let today = Local::now().date_naive();
    if today > sprint.end_date {
        let ago = (today - sprint.end_date).num_days();
        let warn = Style::default().fg(Color::Yellow);
        return first_fitting(vec![
            vec![name.clone(), Span::styled(format!(" ended {ago}d ago   S starts the next one "), warn)],
            vec![name.clone(), Span::styled(" ended   S: next sprint ", warn)],
            vec![Span::styled("sprint ended   S: next ", warn)],
        ]);
    }
    let issues: Vec<&Issue> = app
        .issues
        .iter()
        .filter(|i| i.sprint_id == Some(sprint.id) && i.parent_id.is_none())
        .collect();
    let total: f64 = issues.iter().map(|i| i.story_points).sum();
    let done: f64 = issues.iter().filter(|i| i.status == Status::Done).map(|i| i.story_points).sum();
    let frac = if total > 0.0 { done / total } else { 0.0 };
    let when = if today < sprint.start_date {
        format!("   starts {}", sprint.start_date.format("%a"))
    } else {
        let days = sprint.duration_days();
        format!("   day {}/{days}", ((today - sprint.start_date).num_days() + 1).clamp(1, days))
    };
    let when = Span::styled(when, dim);
    let points = Span::styled(
        format!("   {}/{}sp ", format_sp(done), format_sp(total)),
        Style::default().fg(Color::Magenta),
    );
    let pct = Span::styled(format!(" {:.0}% ", frac * 100.0), dim);
    let mut full = vec![name.clone(), when.clone(), points.clone()];
    full.extend(progress_bar(frac, 12, Color::Green));
    full.push(pct.clone());
    first_fitting(vec![
        full,
        vec![name.clone(), when, points.clone(), pct.clone()],
        vec![name.clone(), points, pct.clone()],
        vec![name, pct],
    ])
}

/// A thin horizontal bar: `width` cells, the first `frac` of them in `color`.
pub fn progress_bar(frac: f64, width: usize, color: Color) -> Vec<Span<'static>> {
    let filled = ((frac.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled("━".repeat(filled), Style::default().fg(color)),
        Span::styled("─".repeat(width - filled), Style::default().fg(Color::DarkGray)),
    ]
}

/// Color for a due date: red when overdue, yellow within a week, dim otherwise
/// (and always dim once the issue is done).
pub fn due_color(due: chrono::NaiveDate, done: bool) -> Color {
    if done {
        return Color::DarkGray;
    }
    let days = (due - Local::now().date_naive()).num_days();
    if days < 0 {
        Color::Red
    } else if days < 7 {
        Color::Yellow
    } else {
        Color::DarkGray
    }
}

/// "today", "tomorrow", "in 3d", "2d overdue".
fn due_relative(due: chrono::NaiveDate) -> String {
    match (due - Local::now().date_naive()).num_days() {
        0 => "today".into(),
        1 => "tomorrow".into(),
        d if d > 1 => format!("in {d}d"),
        d => format!("{}d overdue", -d),
    }
}

/// Split `text` into lines that fit within `width` characters, breaking on word boundaries.
pub fn word_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let (cur_len, word_len) = (current.chars().count(), word.chars().count());
        if current.is_empty() {
            current.push_str(word);
        } else if cur_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Lines of the detail panel for `issue` (shared by the backlog and kanban views),
/// for a panel `width` columns wide.
fn detail_lines(app: &App, issue: &Issue, width: u16) -> Vec<Line<'static>> {
    let dim = Style::default().fg(Color::DarkGray);
    let sc = status_color(&issue.status);
    let is_done = issue.status == Status::Done;

    let mut header = vec![
        Span::styled(format!("  {} ", status_symbol(&issue.status)), Style::default().fg(sc)),
        Span::styled(issue.title.clone(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(
                "   #{} · {}sp · {} · {}",
                issue.id,
                format_sp(issue.story_points),
                issue.status.label(),
                issue.epic
            ),
            dim,
        ),
    ];
    if issue.carry_count > 0 {
        header.push(Span::styled(
            format!("  ↩{} carried", issue.carry_count),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    }
    let mut lines = vec![Line::from(header)];

    // Today's frog comes right under the title, so a short panel never hides it.
    if app.frog_id == Some(issue.id) {
        let frog = Style::default().fg(FROG);
        let text = if is_done {
            "done. The next one comes tomorrow.".to_string()
        } else {
            let waiting = (Local::now().naive_local() - issue.created_at).num_days();
            let mut why = vec![format!("open {waiting}d")];
            if issue.carry_count > 0 {
                why.insert(0, format!("carried {}×", issue.carry_count));
            }
            format!("do this one first ({})", why.join(", "))
        };
        lines.push(Line::from(vec![
            Span::styled("  frog ", frog.add_modifier(Modifier::BOLD)),
            Span::styled(text, frog),
        ]));
    }

    // Wrapped by hand so continuation lines keep the indent.
    if let Some(desc) = issue.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        let text_w = (width as usize).saturating_sub(2 + 4 + 1);
        for text in desc.lines() {
            for chunk in word_wrap(text, text_w) {
                lines.push(Line::from(Span::styled(format!("    {chunk}"), dim)));
            }
        }
    }

    let fmt = |d: chrono::NaiveDateTime| d.format("%Y-%m-%d %H:%M").to_string();
    let mut meta = vec![Span::styled("  due ", dim)];
    match issue.due_date {
        Some(d) => meta.push(Span::styled(
            format!("{} ({})", d.format("%Y-%m-%d"), due_relative(d)),
            Style::default().fg(if is_done { Color::DarkGray } else { due_color(d, false) }),
        )),
        None => meta.push(Span::styled("—", dim)),
    }
    // How long the issue was / has been in progress
    if let Some(start) = issue.started_at {
        let end = issue.completed_at.unwrap_or_else(|| Local::now().naive_local());
        let label = if is_done { "actual" } else { "ongoing" };
        meta.push(Span::styled("   time ", dim));
        meta.push(Span::styled(
            format!("{} {label}", format_duration(start, end)),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(done) = issue.completed_at {
        meta.push(Span::styled(format!("   done {}", fmt(done)), dim));
    }
    meta.push(Span::styled(format!("   created {}", issue.created_at.format("%Y-%m-%d")), dim));
    meta.push(Span::styled(format!("   updated {}", fmt(issue.updated_at)), dim));
    lines.push(Line::from(meta));
    lines
}

/// Rows a list of lines takes when wrapped to `width` columns.
fn wrapped_height(lines: &[Line], width: u16) -> u16 {
    let w = width.max(1) as usize;
    lines.iter().map(|l| l.width().max(1).div_ceil(w) as u16).sum()
}

/// Height (with borders) the detail panel needs for `issue`, capped at `max`.
pub fn detail_height(app: &App, issue: Option<&Issue>, width: u16, max: u16) -> u16 {
    let content = match issue {
        Some(issue) => wrapped_height(&detail_lines(app, issue, width), width.saturating_sub(2)),
        None => 1,
    };
    (content + 2).clamp(4, max.max(4))
}

/// The detail panel: title, description, dates and time spent for one issue.
pub fn render_issue_detail(f: &mut Frame, app: &App, issue: Option<&Issue>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_DIM))
        .title(Span::styled(" detail ", Style::default().fg(Color::DarkGray)));
    let Some(issue) = issue else {
        f.render_widget(
            Paragraph::new("  No issue selected.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    };
    f.render_widget(
        Paragraph::new(detail_lines(app, issue, area.width)).wrap(Wrap { trim: false }).block(block),
        area,
    );
}

/// The bottom hint of a panel: " [n] new  [e] edit … " in dim text.
pub fn hint(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {text} "), Style::default().fg(Color::DarkGray)))
}
