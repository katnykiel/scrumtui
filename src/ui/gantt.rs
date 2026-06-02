use chrono::{Duration, Local, NaiveDate};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use crate::models::{format_sp, Status};
use crate::ui::backlog::status_color;

const BAR_WIDTH: usize = 26;

// Indent before the sp+bar on line 2 of each issue row.
// "    " (4 lead) + "{:>4}sp  " (8) = 12 chars before "["
// We keep line-1 title at 4-char lead too so columns feel aligned.
const BAR_LEAD: usize = 12; // chars before the opening "["

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    if app.issues.is_empty() {
        f.render_widget(
            Paragraph::new("  No issues yet. Go to the backlog (1) and press n to create one.")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" epic / gantt ")
                        .border_style(Style::default().fg(Color::Rgb(80, 80, 120))),
                ),
            area,
        );
        return;
    }

    let today = Local::now().date_naive();

    // Only show top-level issues (no children) in gantt
    let issues: Vec<_> = app.issues.iter().filter(|i| i.parent_id.is_none()).collect();

    let bar_end_of = |i: &&crate::models::Issue| -> NaiveDate {
        if i.status == crate::models::Status::Done {
            i.completed_at.map(|dt| dt.date()).or(i.due_date).unwrap_or(today)
        } else {
            i.due_date.unwrap_or(today)
        }
    };

    let timeline_start = issues.iter()
        .map(|i| i.created_at.date())
        .min()
        .unwrap_or(today - Duration::days(1));
    let timeline_end = issues.iter()
        .map(|i| bar_end_of(i))
        .max()
        .unwrap_or(today + Duration::days(14));
    // Ensure at least a few days of range and the timeline doesn't run backwards
    let timeline_end = timeline_end.max(timeline_start + Duration::days(3));
    let total_days = (timeline_end - timeline_start).num_days().max(1) as usize;

    // Available inner width (subtract 2 for block borders)
    let inner_width = (area.width as usize).saturating_sub(2);
    // Max title length: whatever is left after bar + metadata on line 2
    // line-2 budget: BAR_LEAD + 1("[") + BAR_WIDTH + 1("]") + " " + dates(15) + "  " + status(11) = ~56
    // Line-1 title can use the full inner width, but we cap generously to keep it readable
    let title_max = inner_width.saturating_sub(4); // 4-char lead indent

    let epics = app.epics();
    let mut all_lines: Vec<Line<'static>> = vec![];

    // ── Timeline header ────────────────────────────────────────────────────────
    all_lines.push(build_timeline_header(timeline_start, timeline_end, inner_width));
    all_lines.push(Line::from(Span::styled(
        "─".repeat(inner_width),
        Style::default().fg(Color::DarkGray),
    )));

    for epic in &epics {
        let epic_issues: Vec<_> = issues.iter().filter(|i| &i.epic == epic && i.parent_id.is_none()).collect();
        if epic_issues.is_empty() {
            continue;
        }

        all_lines.push(Line::from(Span::styled(
            format!("  ◆ {}", epic.to_uppercase()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        for issue in &epic_issues {
            let bar_start = issue.created_at.date();
            let bar_end = bar_end_of(issue);

            let bar = build_bar(bar_start, bar_end, timeline_start, total_days, &issue.status);
            let sc = status_color(&issue.status);

            // ── Line 1: title ──────────────────────────────────────────────
            all_lines.push(Line::from(vec![
                Span::styled(
                    format!("    {}", crate::ui::backlog::trunc(&issue.title, title_max)),
                    Style::default().fg(Color::Rgb(200, 200, 200)),
                ),
            ]));

            // ── Line 2: sp + bar + dates + status ──────────────────────────
            let date_str = format!(
                "  {} → {}",
                bar_start.format("%b %d"),
                bar_end.format("%b %d")
            );
            all_lines.push(Line::from(vec![
                Span::styled(
                    format!("    {:>4}sp  ", format_sp(issue.story_points)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(bar, Style::default().fg(sc)),
                Span::styled(date_str, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("  {}", issue.status.label()),
                    Style::default().fg(sc).add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        all_lines.push(Line::from("")); // spacer between epics
    }

    let scroll = app.gantt_scroll.min(all_lines.len().saturating_sub(1));
    let visible: Vec<Line<'static>> = all_lines.into_iter().skip(scroll).collect();

    let para = Paragraph::new(visible).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "epic / gantt",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
            ]))
            .title_bottom(Line::from(Span::styled(
                " [j/k] scroll  [1]backlog [2]kanban  [?]help ",
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
    status: &Status,
) -> String {
    let bar_start_days = (start - timeline_start).num_days().max(0) as usize;
    let duration_days = (end - start).num_days().max(1) as usize;

    let bar_start_col = bar_start_days * BAR_WIDTH / total_days;
    let bar_len = (duration_days * BAR_WIDTH / total_days).max(1);
    let bar_end_col = (bar_start_col + bar_len).min(BAR_WIDTH);

    let fill_char = match status {
        Status::Todo => '░',
        Status::InProgress => '▓',
        Status::Done => '█',
    };

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

    // BAR_LEAD = 12 chars of "    {:>4}sp  " before "["
    let lead = " ".repeat(BAR_LEAD + 4); // +4 for the outer "    " indent
    let mut header = format!("{lead}[");

    let mut bar_chars: Vec<char> = vec![' '; BAR_WIDTH];
    let mut d = start;
    while d <= end {
        let col =
            (d - start).num_days() as usize * BAR_WIDTH / total_days;
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

    // Pad/truncate to inner_width so it never overflows
    if header.len() > inner_width {
        header.truncate(inner_width);
    }

    Line::from(Span::styled(header, Style::default().fg(Color::DarkGray)))
}
