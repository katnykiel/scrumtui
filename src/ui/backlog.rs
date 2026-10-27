use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::app::{App, BacklogItem};
use crate::models::{format_sp, Status};
use crate::ui::{due_color, hint, render_issue_detail, detail_height, BORDER, FROG};

pub fn status_color(status: &Status) -> Color {
    match status {
        Status::Todo       => Color::Yellow,
        Status::InProgress => Color::Cyan,
        Status::Done       => Color::Green,
    }
}

pub fn status_symbol(status: &Status) -> &'static str {
    match status {
        Status::Todo       => "○",
        Status::InProgress => "◉",
        Status::Done       => "✓",
    }
}

/// Width of the epic column.
const EPIC_W: usize = 14;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let show_search = app.search_active || !app.search_query.is_empty();
    let selected = app.selected_issue();
    let detail_h = detail_height(app, selected.as_ref(), area.width, (area.height / 3).max(5));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if show_search { 1 } else { 0 }),
            Constraint::Min(5),
            Constraint::Length(detail_h),
        ])
        .split(area);

    if show_search {
        render_search_bar(f, app, chunks[0]);
    }
    render_list(f, app, chunks[1]);
    render_issue_detail(f, app, selected.as_ref(), chunks[2]);
}

fn render_search_bar(f: &mut Frame, app: &App, area: Rect) {
    let cursor = if app.search_active { "▌" } else { "" };
    let content = format!("  / {}{}", app.search_query, cursor);
    let style = if app.search_active {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let help = if app.search_active { "  [Enter] keep filter  [Esc] clear" } else { "  [Esc] clear filter" };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(content, style),
            Span::styled(help, Style::default().fg(Color::DarkGray)),
        ])),
        area,
    );
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items_data = app.backlog_items();
    let selected = app.backlog_sel;
    let inner_w = area.width.saturating_sub(2) as usize;
    // Title takes what the fixed columns leave: pointer+indent+symbol (7), points (6),
    // status (6), epic (2+14), due date (8) and room for badges (10).
    let title_w = inner_w.saturating_sub(7 + 6 + 6 + 2 + EPIC_W + 8 + 10).clamp(20, 72);
    let today = Local::now().date_naive();

    let items: Vec<ListItem> = items_data
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_sel = idx == selected;
            let pointer = if is_sel { "▶" } else { " " };
            match item {
                BacklogItem::SprintHeader(sprint) => {
                    let mut spans = vec![Span::styled(
                        format!(
                            "  ┌─ {} · {} → {}",
                            sprint.name,
                            sprint.start_date.format("%b %d"),
                            sprint.end_date.format("%b %d, %Y"),
                        ),
                        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                    )];
                    if today > sprint.end_date {
                        spans.push(Span::styled(
                            "  ended: S starts the next sprint",
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                }
                BacklogItem::SprintFooter => {
                    let dashes = "─".repeat(inner_w.saturating_sub(3));
                    ListItem::new(Line::from(Span::styled(
                        format!("  └{dashes}"),
                        Style::default().fg(Color::Magenta),
                    )))
                }
                BacklogItem::BacklogHeader => {
                    let dashes = "─".repeat(inner_w.saturating_sub(14));
                    ListItem::new(Line::from(Span::styled(
                        format!("  ── backlog {dashes}"),
                        Style::default().fg(Color::DarkGray),
                    )))
                }
                BacklogItem::Issue(issue, in_sprint) => {
                    let sc = status_color(&issue.status);
                    let is_done = issue.status == Status::Done;
                    let indent = if *in_sprint { " │  " } else { "    " };
                    let due = issue.due_date
                        .map(|d| format!("  {}", d.format("%b %d")))
                        .unwrap_or_default();
                    let due_style = Style::default().fg(
                        issue.due_date.map(|d| due_color(d, is_done)).unwrap_or(Color::DarkGray),
                    );
                    // Active: terminal default fg (readable on light and dark themes).
                    // Done: Gray, visibly faded on both.
                    let title_style = match (is_sel, is_done) {
                        (true, _) => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                        (false, true) => Style::default().fg(Color::Gray),
                        (false, false) => Style::default(),
                    };
                    let bold = if is_sel { Modifier::BOLD } else { Modifier::empty() };

                    let mut spans = vec![
                        Span::styled(pointer, Style::default().fg(Color::Magenta)),
                        Span::styled(indent, Style::default().fg(Color::Magenta)),
                        Span::styled(format!("{} ", status_symbol(&issue.status)), Style::default().fg(sc)),
                        Span::styled(format!("{:<title_w$}", trunc(&issue.title, title_w)), title_style),
                        Span::styled(
                            format!(" {:>3}sp", format_sp(issue.story_points)),
                            Style::default().fg(Color::Magenta).add_modifier(bold),
                        ),
                        Span::styled(format!("  {:<4}", issue.status.short()), Style::default().fg(sc).add_modifier(bold)),
                        Span::styled(
                            format!("  {:<EPIC_W$}", trunc(&issue.epic, EPIC_W)),
                            Style::default().fg(Color::Cyan).add_modifier(bold),
                        ),
                        Span::styled(due, due_style),
                    ];
                    // Carry-over badge: how many sprints this issue has been carried through.
                    if issue.carry_count > 0 {
                        spans.push(Span::styled(
                            format!("  ↩{}", issue.carry_count),
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ));
                    }
                    if app.frog_id == Some(issue.id) {
                        spans.push(Span::styled("  frog", Style::default().fg(FROG).add_modifier(Modifier::BOLD)));
                    }
                    ListItem::new(Line::from(spans))
                }
                BacklogItem::Subtask(sub, in_sprint) => {
                    let sc = status_color(&sub.status);
                    let indent = if *in_sprint { " │     └ " } else { "       └ " };
                    // Subtasks: never bold, so they read as subordinate to their parent.
                    let title_style = match (is_sel, sub.status == Status::Done) {
                        (true, _) => Style::default().add_modifier(Modifier::REVERSED),
                        (false, true) => Style::default().fg(Color::Gray),
                        (false, false) => Style::default(),
                    };
                    // The indent is 5 wider than a parent's, so the title is 5 narrower
                    // and the status lines up with the parents' status column.
                    let sub_w = title_w.saturating_sub(5);
                    ListItem::new(Line::from(vec![
                        Span::styled(pointer, Style::default().fg(Color::Magenta)),
                        Span::styled(indent, Style::default().fg(Color::DarkGray)),
                        Span::styled(format!("{} ", status_symbol(&sub.status)), Style::default().fg(sc)),
                        Span::styled(format!("{:<sub_w$}", trunc(&sub.title, sub_w)), title_style),
                        Span::styled(format!("        {:<4}", sub.status.short()), Style::default().fg(sc)),
                    ]))
                }
            }
        })
        .collect();

    let hint_text = if app.search_query.is_empty() {
        "[n] new  [e] edit  [h/l] status  [s] sprint  [/] search  [u] undo  [?] help"
    } else {
        "[Esc] clear filter  [n] new  [e] edit  [h/l] status  [u] undo  [?] help"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title_bottom(hint(hint_text))
        .border_style(Style::default().fg(BORDER));

    if items.is_empty() {
        let text = if app.search_query.is_empty() {
            "  Nothing here yet. Press n to create an issue."
        } else {
            "  No issues match the search."
        };
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default()
        .with_offset(app.backlog_offset)
        .with_selected(Some(selected));
    let list = List::new(items).block(block).scroll_padding(2);
    f.render_stateful_widget(list, area, &mut state);
    app.backlog_offset = state.offset();
}

pub fn trunc(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        format!("{}…", chars[..max.saturating_sub(1)].iter().collect::<String>())
    }
}
