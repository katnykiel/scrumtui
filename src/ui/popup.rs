use chrono::{Local, NaiveDateTime};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Chart, Clear, Dataset, GraphType, List, ListItem,
        Paragraph, Wrap,
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
        Popup::EditIssue(form) => {
            let title: String = match form.editing_id {
                Some(id) => format!("Edit Issue  #{id}"),
                None => "Edit Issue".to_string(),
            };
            render_issue_form(f, form, &title, app);
        }
        Popup::SprintManager(form) => render_sprint_form(f, form, app),
        Popup::ConfirmDelete(_, title) => render_confirm_delete(f, title),
        Popup::ConfirmDeleteSprint(_, name) => render_confirm_delete_sprint(f, name),
        Popup::Trash { items, sel } => render_trash(f, items, *sel),
        Popup::Help => render_help(f),
        Popup::GanttEpicDetail { .. } => {} // handled separately in mod.rs
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
    "Epic",
    "Story Points",
    "Status",
    "Due Date (YYYY-MM-DD)",
    "Description",
];

fn render_issue_form(f: &mut Frame, form: &IssueForm, title: &str, app: &App) {
    let visible_subtasks = form.subtasks.iter().filter(|s| !s.deleted).count();
    // header + each subtask row, minimum 3 rows so the section is always visible
    let subtask_section_height: u16 = (2 + visible_subtasks).max(3) as u16;
    // Description field gets more rows when focused so text wraps visibly
    let desc_height: u16 = if form.focused_field == 5 { 5 } else { 3 };
    let base_height: u16 = 19 + desc_height;
    let total_height = (base_height + subtask_section_height).min(f.area().height.saturating_sub(2));
    let area = centered_rect(72, total_height, f.area());
    f.render_widget(Clear, area);

    let bottom_hint = if form.in_subtask_list {
        " [j/k] nav  [e] edit  []/[ status  [x] del  [Ctrl+N] add  [Esc] back  [Enter] save "
    } else if form.status_dropdown_open {
        " [j/k] select  [Enter] confirm  [Esc] close "
    } else if form.focused_field == 5 {
        " [Tab] next field  [Enter] save  [Esc] cancel "
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
            Constraint::Length(desc_height), // desc (taller when focused)
            Constraint::Length(2), // error
            Constraint::Min(subtask_section_height), // subtasks
        ])
        .split(inner);

    // When description is focused show raw text (with newlines); otherwise show collapsed preview
    let desc_display = if form.focused_field == 5 {
        form.description.clone()
    } else {
        form.description.replace('\n', "  ·  ")
    };
    let values: [String; 6] = [
        form.title.clone(),
        form.epic.clone(),
        form.story_points.clone(),
        Status::from_index(form.status_idx).label().to_string(),
        form.due_date.clone(),
        desc_display,
    ];

    for i in 0..6 {
        // Don't show any field as focused when the subtask section has focus
        let is_focused = form.focused_field == i && !form.in_subtask_list;
        let label = FIELD_LABELS[i];

        let value_display = if i == 3 {
            // Status: show current value with dropdown hint
            let arrow = if is_focused { "  ▼" } else { "" };
            format!(" {}{}", values[i], arrow)
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
                Style::default().fg(sc).add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(sc).add_modifier(Modifier::BOLD)
            }
        } else {
            field_style
        };

        if i == 5 {
            // Description: multi-line with word wrap.
            // When focused, use the terminal's real cursor instead of REVERSED highlight
            // (REVERSED on a multi-line block looks broken in dark/night-mode terminals).
            let mut desc_lines: Vec<Line> = vec![
                Line::from(Span::styled(format!(" {label}"), label_style)),
            ];
            for text_line in value_display.split('\n') {
                desc_lines.push(Line::from(Span::styled(
                    format!(" {}", text_line),
                    if is_focused { Style::default() } else { value_style },
                )));
            }
            f.render_widget(
                Paragraph::new(desc_lines)
                    .wrap(Wrap { trim: false })
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
            // Place the real terminal cursor at the end of the last content line
            if is_focused {
                let last_line = value_display.split('\n').last().unwrap_or("");
                let last_idx = value_display.split('\n').count().saturating_sub(1);
                // +1 for label row, area y offset; +1 inside block indent, +1 for " " prefix
                let cx = (field_areas[i].x + 1 + 1 + last_line.chars().count() as u16)
                    .min(field_areas[i].x + field_areas[i].width.saturating_sub(2));
                let cy = (field_areas[i].y + 1 + last_idx as u16)
                    .min(field_areas[i].y + field_areas[i].height.saturating_sub(1));
                f.set_cursor_position((cx, cy));
            }
        } else {
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
        render_epic_dropdown(f, form, app, field_areas[1]);
    }

    // ── Due-date autocomplete dropdown ─────────────────────────────────────────
    if form.due_date_dropdown_open {
        render_due_date_dropdown(f, form, app, field_areas[4]);
    }

    // ── Status dropdown ─────────────────────────────────────────────────────────
    if form.status_dropdown_open {
        render_status_dropdown(f, form, field_areas[3]);
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
            let status_label = status.short();
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

fn render_status_dropdown(f: &mut Frame, form: &IssueForm, field_area: Rect) {
    const OPTIONS: [(&str, Color); 3] = [
        ("Todo",        Color::Yellow),
        ("In Progress", Color::Cyan),
        ("Done",        Color::Green),
    ];

    let height = OPTIONS.len() as u16 + 2;
    let width = 16u16;
    let drop_area = Rect {
        x: field_area.x + 2,
        y: field_area.y + field_area.height,
        width,
        height,
    };

    f.render_widget(Clear, drop_area);

    let sel = form.status_dropdown_sel;
    let items: Vec<ListItem> = OPTIONS
        .iter()
        .enumerate()
        .map(|(i, (label, color))| {
            let is_sel = i == sel;
            ListItem::new(Line::from(Span::styled(
                format!(" {label}"),
                if is_sel {
                    Style::default().fg(*color).add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default().fg(*color)
                },
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(" status ", Style::default().fg(Color::DarkGray))),
    );

    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(sel));
    f.render_stateful_widget(list, drop_area, &mut state);
}

fn render_epic_dropdown(f: &mut Frame, form: &IssueForm, app: &App, epic_field_area: Rect) {
    let q = form.epic.to_lowercase();
    let matches: Vec<&String> = app
        .epics()
        .iter()
        .filter(|e| e.to_lowercase().contains(&q))
        .collect();

    if matches.is_empty() {
        return;
    }

    let max_items = 6usize;
    let visible: Vec<&&String> = matches.iter().take(max_items).collect();
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

fn render_due_date_dropdown(f: &mut Frame, form: &IssueForm, app: &App, field_area: Rect) {
    let q = form.due_date.to_lowercase();
    let matches: Vec<String> = app
        .due_dates()
        .into_iter()
        .filter(|d| d.contains(&q))
        .collect();

    if matches.is_empty() {
        return;
    }

    let max_items = 6usize;
    let visible: Vec<&String> = matches.iter().take(max_items).collect();
    let height = visible.len() as u16 + 2;
    let width = (visible.iter().map(|d| d.len()).max().unwrap_or(10) + 4)
        .max(20)
        .min(field_area.width as usize) as u16;

    let drop_area = Rect {
        x: field_area.x + 2,
        y: field_area.y + field_area.height,
        width,
        height,
    };

    f.render_widget(Clear, drop_area);

    let sel = form.due_date_dropdown_sel.min(visible.len().saturating_sub(1));
    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(i, date)| {
            let is_sel = i == sel;
            let label = if app.today_str() == **date {
                format!(" {date}  ← today")
            } else {
                format!(" {date}")
            };
            ListItem::new(Line::from(Span::styled(
                label,
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
                .title(Span::styled(" due dates ", Style::default().fg(Color::DarkGray))),
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

    // ── Right: burnup chart ───────────────────────────────────────────────────
    let chart_area = h_chunks[1];
    if let Some(sprint_id) = form.editing_id {
        let start = chrono::NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").ok();
        let end   = chrono::NaiveDate::parse_from_str(&form.end_date,   "%Y-%m-%d").ok();
        if let (Some(start), Some(end)) = (start, end) {
            let sprint_issues: Vec<&Issue> = app
                .issues
                .iter()
                .filter(|i| i.sprint_id == Some(sprint_id))
                .collect();
            render_burnup_chart(f, start, end, &sprint_issues, chart_area);
            return;
        }
    }
    // Fallback: no sprint selected yet
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(Span::styled(" burnup ", Style::default().fg(Color::DarkGray)))
                .border_style(Style::default().fg(Color::Rgb(80, 60, 100))),
        ),
        chart_area,
    );
}

/// Compute burnup chart data from a sprint's date range and its issues.
///
/// Returns (scope_line, ideal_line, actual_line, total_sp, sprint_days) where:
/// - scope_line: step function — starts at initial SP, steps up as issues are added mid-sprint
/// - ideal_line: straight line from (0, 0) to (sprint_days, total_sp) — perfect steady pace
/// - actual_line: completed SP step function — starts at 0, steps up as issues are marked Done
///
/// All three lines have non-negative slopes. X axis is fractional days from sprint start.
pub fn compute_burnup_for(
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    issues: &[&Issue],
) -> Option<(Vec<(f64, f64)>, Vec<(f64, f64)>, Vec<(f64, f64)>, f64, f64)> {
    if end <= start {
        return None;
    }

    let total_sp: f64 = issues.iter().map(|i| i.story_points).sum();
    if total_sp <= 0.0 {
        return None;
    }

    let sprint_days = (end - start).num_days() as f64;
    let start_dt: NaiveDateTime = start.and_hms_opt(0, 0, 0).unwrap();

    // Scope line: step function that increases as issues are added during the sprint.
    // Issues created before sprint start count from day 0; those created mid-sprint
    // add their SP at their creation timestamp.
    let now = Local::now().naive_local();
    let sprint_end_dt: NaiveDateTime = end.and_hms_opt(23, 59, 59).unwrap();
    let scope = {
        // Separate issues into "already in scope at start" vs "added mid-sprint"
        let mut initial_sp: f64 = 0.0;
        let mut additions: Vec<(f64, f64)> = Vec::new();
        for i in issues.iter() {
            let frac = (i.created_at - start_dt).num_seconds() as f64 / 86400.0;
            if frac <= 0.0 {
                initial_sp += i.story_points;
            } else {
                additions.push((frac.min(sprint_days), i.story_points));
            }
        }
        additions.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut scope: Vec<(f64, f64)> = Vec::new();
        let mut running = initial_sp;
        scope.push((0.0, running));
        for (x, sp) in &additions {
            scope.push((*x, running));
            running += sp;
            scope.push((*x, running));
        }
        // Extend to current time (or end of sprint)
        let current_x = {
            let cx = (now.min(sprint_end_dt) - start_dt).num_seconds() as f64 / 86400.0;
            cx.clamp(0.0, sprint_days)
        };
        if scope.last().map(|(x, _)| *x).unwrap_or(0.0) < current_x {
            scope.push((current_x, running));
        }
        scope
    };

    // Ideal burnup: straight line from (0, 0) to (sprint_days, total_sp)
    let ideal: Vec<(f64, f64)> = vec![(0.0, 0.0), (sprint_days, total_sp)];

    // Actual burnup: step function of completed story points.
    // Starts at 0 and steps up each time an issue is marked Done.
    let cutoff = now.min(sprint_end_dt);

    // Gather completion events within sprint window
    let mut events: Vec<(f64, f64)> = issues
        .iter()
        .filter_map(|i| {
            if i.status == Status::Done {
                i.completed_at.and_then(|c| {
                    if c <= cutoff {
                        let frac_days = (c - start_dt).num_seconds() as f64 / 86400.0;
                        // Only count completions within the sprint window (0..=sprint_days)
                        if frac_days >= 0.0 {
                            Some((frac_days.min(sprint_days), i.story_points))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .collect();

    // Sort by time
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Build step function: start at 0, add SP at each completion moment
    let mut actual: Vec<(f64, f64)> = Vec::new();
    let mut completed = 0.0_f64;
    actual.push((0.0, completed));
    for (x, sp) in &events {
        // Horizontal segment up to the step, then jump up
        actual.push((*x, completed));
        completed += sp;
        actual.push((*x, completed));
    }
    // Extend to current time (or end of sprint)
    let current_x = {
        let cx = (now - start_dt).num_seconds() as f64 / 86400.0;
        cx.clamp(0.0, sprint_days)
    };
    if actual.last().map(|(x, _)| *x).unwrap_or(0.0) < current_x {
        actual.push((current_x, completed));
    }

    Some((scope, ideal, actual, total_sp, sprint_days))
}

/// Render a burndown chart for any sprint given its dates and issues.
/// Used by both the sprint manager popup and the history view.
pub fn render_burnup_chart(
    f: &mut Frame,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    issues: &[&Issue],
    area: Rect,
) {
    let chart_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            " burnup ",
            Style::default().fg(Color::DarkGray),
        ))
        .border_style(Style::default().fg(Color::Rgb(80, 60, 100)));

    match compute_burnup_for(start, end, issues) {
        None => {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled(
                        "  No story points in sprint.",
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
                .block(chart_block),
                area,
            );
        }
        Some((scope, ideal, actual, total_sp, sprint_days)) => {
            let y_max = total_sp * 1.1;
            let x_max = sprint_days;

            let mid_day = (sprint_days / 2.0).round() as i64;
            let x_labels = vec![
                Span::styled("d0", Style::default().fg(Color::DarkGray)),
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
                Span::styled("0", Style::default().fg(Color::Green)),
                Span::styled(
                    format_sp(total_sp / 2.0),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format_sp(total_sp),
                    Style::default().fg(Color::Yellow),
                ),
            ];

            let datasets = vec![
                Dataset::default()
                    .name("scope")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Yellow))
                    .data(&scope),
                Dataset::default()
                    .name("ideal")
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(Color::Rgb(120, 120, 120)))
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
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Move to Trash",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
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

fn render_confirm_delete_sprint(f: &mut Frame, name: &str) {
    let area = centered_rect(60, 8, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Delete Sprint",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Delete: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            "  Issues will be unlinked (not deleted).",
            Style::default().fg(Color::DarkGray),
        )),
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
        .border_style(Style::default().fg(Color::Rgb(80, 80, 120)));

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
        key("j / k", "Navigate up / down"),
        key("h / l", "Advance / regress status"),
        key("e", "Edit selected issue"),
        key("n", "New issue"),
        key("u  /  ?", "Undo  /  Help"),
        key("q  /  Esc", "Quit  /  Cancel / back"),
        key("1 / 2 / 3 / 4", "Backlog / Kanban / Gantt / History"),
        sep(),
        hdr("BACKLOG"),
        key("d  /  T", "Trash issue  /  open trash"),
        key("s  /  S", "Toggle sprint  /  sprint manager"),
        key("c", "Toggle show completed"),
        key("/", "Search"),
        sep(),
        hdr("KANBAN"),
        key("[ / ]", "Switch column left / right"),
        key("h / l", "Regress / advance status"),
        key("Tab", "Parent ↔ subtask panel"),
        key("< / >", "Cycle parent in subtask panel"),
        sep(),
        hdr("FORMS"),
        key("Tab / Shift-Tab", "Next / previous field"),
        key("h / l", "Regress / advance subtask status"),
        key("Del", "Clear due date field"),
        key("Ctrl-N", "Add subtask"),
        key("x", "Remove subtask"),
        key("Ctrl-S", "Save from any field"),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}
