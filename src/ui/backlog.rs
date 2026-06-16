use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, BacklogItem};
use crate::models::{format_sp, Status};

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

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let show_search = app.search_active || !app.search_query.is_empty();
    let constraints = if show_search {
        vec![Constraint::Length(1), Constraint::Min(5), Constraint::Length(5)]
    } else {
        vec![Constraint::Min(5), Constraint::Length(5)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    if show_search {
        render_search_bar(f, app, chunks[0]);
        render_list(f, app, chunks[1]);
        render_detail(f, app, chunks[2]);
    } else {
        render_list(f, app, chunks[0]);
        render_detail(f, app, chunks[1]);
    }
}

fn render_search_bar(f: &mut Frame, app: &App, area: Rect) {
    let cursor = if app.search_active { "▌" } else { "" };
    let content = format!("  / {}{}", app.search_query, cursor);
    let style = if app.search_active {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(content, style),
            Span::styled(
                "  [Esc] clear  [Enter] confirm",
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        area,
    );
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items_data = app.backlog_items();
    let selected = app.backlog_sel;

    let items: Vec<ListItem> = items_data
        .iter()
        .enumerate()
        .map(|(idx, item)| match item {
            BacklogItem::SprintHeader(sprint) => {
                let label = format!(
                    "  ┌─ {} · {} → {} ",
                    sprint.name,
                    sprint.start_date.format("%b %d"),
                    sprint.end_date.format("%b %d, %Y"),
                );
                ListItem::new(Line::from(Span::styled(
                    label,
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                )))
            }
            BacklogItem::SprintFooter => {
                let inner_w = area.width.saturating_sub(2) as usize;
                let dashes = "─".repeat(inner_w.saturating_sub(3));
                ListItem::new(Line::from(Span::styled(
                    format!("  └{dashes}"),
                    Style::default().fg(Color::Magenta),
                )))
            }
            BacklogItem::BacklogHeader => {
                let inner_w = area.width.saturating_sub(2) as usize;
                let dashes = "─".repeat(inner_w.saturating_sub(14));
                ListItem::new(Line::from(Span::styled(
                    format!("  ── backlog {dashes}"),
                    Style::default().fg(Color::DarkGray),
                )))
            }
            BacklogItem::Issue(issue, in_sprint) => {
                let sym = status_symbol(&issue.status);
                let sc = status_color(&issue.status);
                let indent = if *in_sprint { "  │  " } else { "     " };
                let due = issue.due_date
                    .map(|d| format!("  {}", d.format("%b %d")))
                    .unwrap_or_default();
                let due_color = issue.due_date.map(|d| {
                    if issue.status == Status::Done {
                        Color::DarkGray
                    } else {
                        let days = (d - chrono::Local::now().date_naive()).num_days();
                        if days < 0 { Color::Red } else if days < 7 { Color::Yellow } else { Color::DarkGray }
                    }
                }).unwrap_or(Color::DarkGray);
                let pointer = if idx == selected { "▶" } else { " " };

                // Carry-over badge — orange/bold only for items that have been carried
                let carry_badge = if issue.carry_count > 0 {
                    format!(" ↩{}", issue.carry_count)
                } else {
                    String::new()
                };

                // Active: terminal default fg (dark on light bg, bright on dark bg) + bold for parents.
                // Done:   DarkGray — visibly faded on both light and dark themes.
                let title_style = if issue.status == Status::Done {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default()
                };
                let epic_style = Style::default().fg(Color::Cyan);
                let sp_style = Style::default().fg(Color::Magenta);

                ListItem::new(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::Magenta)),
                    Span::styled(indent, Style::default().fg(Color::Magenta)),
                    Span::styled(format!("{sym} "), Style::default().fg(sc)),
                    Span::styled(format!("{:<42}", trunc(&issue.title, 42)), title_style),
                    Span::styled(format!(" {:>4}sp", format_sp(issue.story_points)), sp_style),
                    Span::styled(format!("  {:<4}", issue.status.short()), Style::default().fg(sc)),
                    Span::styled(format!("  {:<14}", trunc(&issue.epic, 14)), epic_style),
                    Span::styled(due, Style::default().fg(due_color)),
                    Span::styled(carry_badge, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]))
            }
            BacklogItem::Subtask(sub, in_sprint) => {
                let sym = status_symbol(&sub.status);
                let sc = status_color(&sub.status);
                let indent = if *in_sprint { "  │     └ " } else { "        └ " };
                let pointer = if idx == selected { "▶" } else { " " };
                // Subtasks: terminal default fg for active, DarkGray for done.
                // Not bold — subtler than parent issues to show hierarchy.
                let title_style = if sub.status == Status::Done {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::Magenta)),
                    Span::styled(indent, Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{sym} "), Style::default().fg(sc)),
                    Span::styled(format!("{:<42}", trunc(&sub.title, 42)), title_style),
                    Span::styled(format!("  {:<4}", sub.status.short()), Style::default().fg(sc)),
                ]))
            }
        })
        .collect();

    let mut state = app.backlog_list_state();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("scrumtui", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                ]))
                .title_bottom(Line::from(Span::styled(
                    " [n]ew  [e]dit  [d]elete  []/[]status  [s]print  [S]mgr  [/]search  [c]done  [^j/^k]rank  [u]ndo  [?]help ",
                    Style::default().fg(Color::DarkGray),
                )))
                .border_style(Style::default().fg(Color::Rgb(100, 100, 160))),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));

    f.render_stateful_widget(list, area, &mut state);
}

fn render_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(90, 90, 140)))
        .title(Span::styled(" detail ", Style::default().fg(Color::DarkGray)));

    let Some(issue) = app.selected_issue() else {
        f.render_widget(
            Paragraph::new("  No issue selected.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    };

    let sc = status_color(&issue.status);
    let sym = status_symbol(&issue.status);
    let desc = issue.description.as_deref().unwrap_or("");
    let due_str = issue.due_date
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "—".into());
    let created = issue.created_at.format("%Y-%m-%d %H:%M").to_string();
    let updated = issue.updated_at.format("%Y-%m-%d %H:%M").to_string();
    let completed = issue.completed_at
        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".into());

    let carry_str = if issue.carry_count > 0 {
        format!("  ↩{} carried", issue.carry_count)
    } else {
        String::new()
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("  {sym} "), Style::default().fg(sc)),
            Span::styled(
                issue.title.clone(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   #{} · {}sp · {} · {}", issue.id, format_sp(issue.story_points), issue.status.label(), issue.epic),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(carry_str, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
    ];
    if !desc.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("         ", Style::default()),
            Span::styled(desc, Style::default().fg(Color::DarkGray)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("  due ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<12}", due_str), Style::default().fg(Color::Yellow)),
        Span::styled("  created ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", created), Style::default().fg(Color::DarkGray)),
        Span::styled("  updated ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<17}", updated), Style::default().fg(Color::DarkGray)),
        Span::styled("  done ", Style::default().fg(Color::DarkGray)),
        Span::styled(completed, Style::default().fg(Color::DarkGray)),
    ]));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

pub fn trunc(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        format!("{}…", chars[..max.saturating_sub(1)].iter().collect::<String>())
    }
}
