use chrono::{Duration, Local};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Chart, Clear, Dataset, GraphType, List, ListItem,
        Paragraph,
    },
    Frame,
};

use crate::app::{App, IssueForm, Popup, SprintForm};
use crate::models::{format_sp, Issue, Status};
use crate::ui::backlog::{status_color, status_symbol};

/// Render whichever popup is active on top of the existing frame.
pub fn render(f: &mut Frame, popup: &Popup, app: &App) {
    match popup {
        Popup::NewIssue(form) => render_issue_form(f, form, "New Issue", app),
        Popup::EditIssue(form) => render_issue_form(f, form, "Edit Issue", app),
        Popup::SprintManager(form) => render_sprint_form(f, form, app),
        Popup::ConfirmDelete(_, title) => render_confirm_delete(f, title),
        Popup::Trash { items, sel } => render_trash(f, items, *sel),
        Popup::Help => render_help(f),
    }
}

// ── Layout helpers ─────────────────────────────────────────────────────────────

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(r.width),
        height: height.min(r.height),
    }
}

// ── Issue form ─────────────────────────────────────────────────────────────────

const FIELD_LABELS: [&str; 6] = [
    "Title",
    "Story Points",
    "Epic",
    "Status",
    "Due Date (YYYY-MM-DD)",
    "Description",
];

fn render_issue_form(f: &mut Frame, form: &IssueForm, title: &str, app: &App) {
    let visible_subtasks = form.subtasks.iter().filter(|s| !s.deleted).count();
    // header + each subtask row, minimum 3 rows so the section is always visible
    let subtask_section_height: u16 = (2 + visible_subtasks).max(3) as u16;
    let base_height: u16 = 22;
    let total_height = (base_height + subtask_section_height).min(f.area().height.saturating_sub(2));
    let area = centered_rect(72, total_height, f.area());
    f.render_widget(Clear, area);

    let bottom_hint = if form.in_subtask_list {
        " [j/k] nav  [e] edit  []/[ status  [x] del  [Ctrl+N] add  [Esc] back  [Enter] save "
    } else {
        " [Tab] next field  [Enter] save  [Esc] cancel "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            bottom_hint,
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Rgb(100, 100, 180)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let field_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title
            Constraint::Length(3), // sp
            Constraint::Length(3), // epic
            Constraint::Length(3), // status
            Constraint::Length(3), // due
            Constraint::Length(3), // desc
            Constraint::Length(2), // error
            Constraint::Min(subtask_section_height), // subtasks
        ])
        .split(inner);

    // For display, show the description on one line (collapse newlines)
    let desc_display = form.description.replace('\n', "  ·  ");
    let values: [String; 6] = [
        form.title.clone(),
        form.story_points.clone(),
        form.epic.clone(),
        Status::from_index(form.status_idx).label().to_string(),
        form.due_date.clone(),
        desc_display,
    ];

    for i in 0..6 {
        // Don't show any field as focused when the subtask section has focus
        let is_focused = form.focused_field == i && !form.in_subtask_list;
        let label = FIELD_LABELS[i];

        let value_display = if i == 3 {
            // Status: show cycling UI
            let prev = if form.status_idx > 0 {
                Status::from_index(form.status_idx - 1).label()
            } else {
                ""
            };
            let next = if form.status_idx < 2 {
                Status::from_index(form.status_idx + 1).label()
            } else {
                ""
            };
            format!(" [h] {}  ◀  {}  ▶  [l] {}", prev, values[i], next)
        } else {
            let cursor = if is_focused { "▌" } else { "" };
            format!(" {}{}", values[i], cursor)
        };

        let field_style = if is_focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label_style = if is_focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let value_style = if i == 3 {
            let s = Status::from_index(form.status_idx);
            let sc = match s {
                Status::Todo => Color::Yellow,
                Status::InProgress => Color::Cyan,
                Status::Done => Color::Green,
            };
            if is_focused {
                Style::default()
                    .fg(sc)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(sc)
            }
        } else {
            field_style
        };

        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(format!(" {label}"), label_style)),
                Line::from(Span::styled(&value_display[..], value_style)),
            ])
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(if is_focused {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            ),
            field_areas[i],
        );
    }

    if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  ⚠ {err}"),
                Style::default().fg(Color::Red),
            ))),
            field_areas[6],
        );
    }

    // ── Subtask list (always shown) ───────────────────────────────────────────────────────
    render_subtask_list(f, form, field_areas[7]);

    // ── Epic autocomplete dropdown ─────────────────────────────────────────────
    if form.epic_dropdown_open {
        render_epic_dropdown(f, form, app, field_areas[2]);
    }
}

fn render_subtask_list(f: &mut Frame, form: &IssueForm, area: Rect) {
    let is_focused = form.in_subtask_list;
    let border_color = if is_focused { Color::Cyan } else { Color::Rgb(60, 60, 90) };
    let label_style = if is_focused {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(vec![
            Span::styled(" Subtasks ", label_style),
            Span::styled(
                if is_focused { "" } else { "[Tab] to focus" },
                Style::default().fg(Color::DarkGray),
            ),
        ]));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    let visible: Vec<_> = form.subtasks.iter().filter(|s| !s.deleted).collect();
    let mut lines: Vec<Line> = Vec::new();

    if visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no subtasks)  — Ctrl+N to add one",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (vis_idx, st) in visible.iter().enumerate() {
            let is_sel = is_focused && vis_idx == form.subtask_sel;
            let status = Status::from_index(st.status_idx);
            let sc = status_color(&status);
            let sym = status_symbol(&status);
            let pointer = if is_sel { "▶" } else { " " };
            // Show cursor only when actively editing the title
            let cursor = if is_sel && form.subtask_editing { "▌" } else { "" };
            let row_style = if is_sel && !form.subtask_editing {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else if is_sel {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let status_label = match status {
                Status::Todo => "TODO",
                Status::InProgress => " IP ",
                Status::Done => "DONE",
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {pointer} "), Style::default().fg(Color::Magenta)),
                Span::styled(format!("{sym} "), Style::default().fg(sc)),
                Span::styled(
                    format!("{}{}", st.title, cursor),
                    row_style,
                ),
                Span::styled(
                    format!("  {status_label}"),
                    Style::default().fg(sc),
                ),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Trash popup ──────────────────────────────────────────────────────────────────

fn render_trash(f: &mut Frame, items: &[crate::models::Issue], sel: usize) {
    let height = (items.len() as u16 + 6).max(10).min(f.area().height.saturating_sub(4));
    let area = centered_rect(68, height, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("🗑  Trash", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            " [r] restore  [D] purge permanently  [Esc] close ",
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  Trash is empty.",
                Style::default().fg(Color::DarkGray),
            ))),
            inner,
        );
        return;
    }

    use crate::ui::backlog::trunc;
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, issue)| {
            let is_sel = i == sel;
            let pointer = if is_sel { "▶" } else { " " };
            let style = if is_sel {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {pointer} "), Style::default().fg(Color::Red)),
                Span::styled(
                    format!("{:<44}", trunc(&issue.title, 44)),
                    style,
                ),
                Span::styled(
                    format!("  {:<14}", trunc(&issue.epic, 14)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(sel));
    let list = List::new(list_items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));
    f.render_stateful_widget(list, inner, &mut state);
}

fn render_epic_dropdown(f: &mut Frame, form: &IssueForm, app: &App, epic_field_area: Rect) {
    let q = form.epic.to_lowercase();
    let matches: Vec<String> = app
        .epics()
        .into_iter()
        .filter(|e| e.to_lowercase().contains(&q))
        .collect();

    if matches.is_empty() {
        return;
    }

    let max_items = 6usize;
    let visible: Vec<&String> = matches.iter().take(max_items).collect();
    let height = visible.len() as u16 + 2; // +2 for border
    let width = (visible.iter().map(|e| e.len()).max().unwrap_or(10) + 4)
        .max(20)
        .min(epic_field_area.width as usize) as u16;

    let drop_area = Rect {
        x: epic_field_area.x + 2,
        y: epic_field_area.y + epic_field_area.height,
        width,
        height,
    };

    f.render_widget(Clear, drop_area);

    let sel = form.epic_dropdown_sel.min(visible.len().saturating_sub(1));
    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(i, epic)| {
            let is_sel = i == sel;
            ListItem::new(Line::from(Span::styled(
                format!(" {epic}"),
                if is_sel {
                    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default()
                },
            )))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(" epics ", Style::default().fg(Color::DarkGray))),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(sel));
    f.render_stateful_widget(list, drop_area, &mut state);
}

// ── Sprint form ────────────────────────────────────────────────────────────────

fn render_sprint_form(f: &mut Frame, form: &SprintForm, app: &App) {
    let area = centered_rect(92, 26, f.area());
    f.render_widget(Clear, area);

    let popup_title = if form.editing_id.is_some() {
        "Edit Sprint"
    } else {
        "New Sprint"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                popup_title,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            " [Tab] next  [Enter] save  [Esc] cancel ",
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner horizontally: left=form, right=burndown chart
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(38), Constraint::Min(10)])
        .split(inner);

    // ── Left: form fields ──────────────────────────────────────────────────────

    let field_constraints: Vec<Constraint> = (0..5)
        .map(|i| {
            if i < 4 {
                Constraint::Length(3)
            } else {
                Constraint::Min(1)
            }
        })
        .collect();

    let field_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(field_constraints)
        .split(h_chunks[0]);

    let fields: [(&str, String, bool); 4] = [
        ("Name", form.name.clone(), false),
        ("Start  (YYYY-MM-DD)", form.start_date.clone(), false),
        ("End    (YYYY-MM-DD)", form.end_date.clone(), false),
        ("Active", if form.is_active { "✓ yes" } else { "✗ no" }.to_string(), true),
    ];

    for (i, (label, value, is_bool)) in fields.iter().enumerate() {
        let is_focused = form.focused_field == i;
        let cursor = if is_focused && !is_bool { "▌" } else { "" };
        let display = if *is_bool {
            format!(" {value}  [space] toggle")
        } else {
            format!(" {value}{cursor}")
        };

        let field_style = if is_focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label_style = if is_focused {
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let bool_style = if *is_bool && form.is_active {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else if *is_bool {
            Style::default().fg(Color::DarkGray)
        } else {
            field_style
        };

        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(format!(" {label}"), label_style)),
                Line::from(Span::styled(
                    &display[..],
                    if *is_bool { bool_style } else { field_style },
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(if is_focused {
                        Style::default().fg(Color::Magenta)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
            ),
            field_chunks[i],
        );
    }

    if let Some(err) = &form.error {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  ⚠ {err}"),
                Style::default().fg(Color::Red),
            ))),
            field_chunks[4],
        );
    }

    // ── Right: burndown chart ──────────────────────────────────────────────────
    render_burndown(f, form, app, h_chunks[1]);
}

/// Compute burnup data for the active sprint.
/// Returns (ideal_line, actual_line, total_sp, sprint_days).
fn compute_burnup(
    form: &SprintForm,
    app: &App,
) -> Option<(Vec<(f64, f64)>, Vec<(f64, f64)>, f64, f64)> {
    let sprint_id = form.editing_id?;
    let start =
        chrono::NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").ok()?;
    let end = chrono::NaiveDate::parse_from_str(&form.end_date, "%Y-%m-%d").ok()?;
    if end <= start {
        return None;
    }

    let sprint_issues: Vec<&Issue> = app
        .issues
        .iter()
        .filter(|i| i.sprint_id == Some(sprint_id))
        .collect();

    let total_sp: f64 = sprint_issues.iter().map(|i| i.story_points).sum();
    if total_sp <= 0.0 {
        return None;
    }

    let sprint_days = (end - start).num_days() as f64;

    // Ideal burnup: straight line from (0,0) to (sprint_days, total_sp)
    let ideal: Vec<(f64, f64)> = vec![(0.0, 0.0), (sprint_days, total_sp)];

    // Actual burnup: step function of cumulative completed story points per day
    let today = Local::now().date_naive();
    let max_day = (today - start)
        .num_days()
        .min((end - start).num_days())
        .max(0);

    let mut actual: Vec<(f64, f64)> = vec![(0.0, 0.0)];
    for d in 1..=max_day {
        let day_date = start + Duration::days(d);
        let cum_sp: f64 = sprint_issues
            .iter()
            .filter(|i| {
                i.status == Status::Done
                    && i.completed_at
                        .map(|c| c.date() <= day_date)
                        .unwrap_or(false)
            })
            .map(|i| i.story_points)
            .sum();
        actual.push((d as f64, cum_sp));
    }

    Some((ideal, actual, total_sp, sprint_days))
}

fn render_burndown(f: &mut Frame, form: &SprintForm, app: &App, area: Rect) {
    let chart_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " burnup ",
            Style::default().fg(Color::DarkGray),
        ))
        .border_style(Style::default().fg(Color::Rgb(80, 60, 100)));

    if form.editing_id.is_none() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No active sprint.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "  Save to create one.",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(chart_block),
            area,
        );
        return;
    }

    match compute_burnup(form, app) {
        None => {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  No sprint issues yet.",
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
                .block(chart_block),
                area,
            );
        }
        Some((ideal, actual, total_sp, sprint_days)) => {
            let y_max = total_sp * 1.1;
            let x_max = sprint_days + 0.5;

            // Build x-axis labels
            let mid_day = (sprint_days / 2.0).round() as i64;
            let x_labels = vec![
                Span::styled("0", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("d{mid_day}"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("d{}", sprint_days as i64),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            let y_labels = vec![
                Span::styled("0", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", format_sp(total_sp / 2.0)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format_sp(total_sp),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            let datasets = vec![
                Dataset::default()
                    .name("ideal")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::DarkGray))
                    .data(&ideal),
                Dataset::default()
                    .name("actual")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&actual),
            ];

            let chart = Chart::new(datasets)
                .block(chart_block)
                .x_axis(
                    Axis::default()
                        .bounds([0.0, x_max])
                        .labels(x_labels)
                        .style(Style::default().fg(Color::DarkGray)),
                )
                .y_axis(
                    Axis::default()
                        .bounds([0.0, y_max])
                        .labels(y_labels)
                        .style(Style::default().fg(Color::DarkGray)),
                );

            f.render_widget(chart, area);
        }
    }
}

// ── Confirm delete ─────────────────────────────────────────────────────────────

fn render_confirm_delete(f: &mut Frame, title: &str) {
    let area = centered_rect(52, 7, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " Move to Trash ",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Trash: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  [d] confirm    [n / Esc] cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

// ── Help ───────────────────────────────────────────────────────────────────────

fn render_help(f: &mut Frame) {
    let area = centered_rect(62, 32, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Help",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            " [Esc / q / ?] close ",
            Style::default().fg(Color::DarkGray),
        )))
        .border_style(Style::default().fg(Color::Rgb(80, 80, 140)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let key = |k: &'static str, desc: &'static str| -> Line<'static> {
        Line::from(vec![
            Span::styled(
                format!("  {k:<22}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc, Style::default()),
        ])
    };
    let sep = || -> Line<'static> { Line::from("") };
    let hdr = |t: &'static str| -> Line<'static> {
        Line::from(Span::styled(
            format!("  ── {t} "),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ))
    };

    let lines = vec![
        sep(),
        hdr("GLOBAL"),
        key("1 / 2 / 3", "Switch view: backlog / kanban / gantt"),
        key("q  Ctrl-C", "Quit"),
        key("?", "Toggle this help"),
        sep(),
        hdr("BACKLOG  (view 1)"),
        key("j / k  ↑ / ↓", "Navigate issues"),
        key("g / G", "Jump to first / last issue"),
        key("> or .  / < or ,", "Advance / regress status"),
        key("n", "New issue"),
        key("e  Enter", "Edit selected issue"),
        key("d", "Move to trash"),
        key("T", "Open trash (restore / purge)"),
        key("s", "Toggle sprint membership"),
        key("S", "Open sprint manager"),
        sep(),
        hdr("KANBAN  (view 2)"),
        key("h / l  ← / →", "Switch column"),
        key("j / k  ↑ / ↓", "Navigate within column"),
        key("> or .", "Advance issue to next status"),
        key("< or ,", "Regress issue to previous status"),
        key("e  Enter", "Edit selected issue"),
        sep(),
        hdr("GANTT  (view 3)"),
        key("j / k", "Scroll down / up"),
        sep(),
        hdr("FORMS"),
        key("Tab / Shift-Tab", "Next / previous field"),
        key("h / l  in status", "Cycle status value"),
        key("Space  in active toggle", "Toggle yes / no"),
        key("Enter", "Save"),
        key("Esc", "Cancel"),
        sep(),
        hdr("DELETE CONFIRM"),
        key("d", "Confirm delete"),
        key("n / Esc", "Cancel"),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}
