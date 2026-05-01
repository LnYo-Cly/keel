use crate::app::{App, DetailTab};
use crate::theme;
use keel_core::{
    ArtifactInfo, CheckResult, CheckStatus, RiskWarning, RiskWarningKind, RunArtifacts,
    RunMetadata, RunStatus,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

const RUN_TABLE_WIDTHS: [Constraint; 5] = [
    Constraint::Length(12),
    Constraint::Length(8),
    Constraint::Length(8),
    Constraint::Min(16),
    Constraint::Length(7),
];
const NARROW_WIDTH: u16 = 110;

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let root = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(theme::BG)), root);
    let header_height = if root.width < NARROW_WIDTH { 2 } else { 5 };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(root);

    render_header(frame, app, layout[0]);
    render_body(frame, app, layout[1]);
    render_footer(frame, app, layout[2]);
    if app.help_visible() {
        render_help_overlay(frame, root);
    }
}

#[cfg(test)]
pub fn render_to_string(app: &mut App, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("test backend should initialize");
    terminal
        .draw(|frame| render(frame, app))
        .expect("test render should draw");
    terminal.backend().to_string()
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width < NARROW_WIDTH {
        render_compact_header(frame, app, area);
        return;
    }

    let counts = app.counts();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(20),
            Constraint::Length(13),
            Constraint::Length(17),
            Constraint::Length(15),
            Constraint::Length(17),
            Constraint::Length(17),
            Constraint::Length(13),
            Constraint::Length(10),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(Span::styled("Keel", theme::title())),
        Line::from(Span::styled(
            "local AI code review",
            Style::default().fg(theme::MUTED).bg(theme::BG),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme::border()),
    )
    .style(Style::default().bg(theme::BG));
    frame.render_widget(title, chunks[0]);

    render_stat(frame, chunks[1], "Ready", counts.ready, theme::GREEN);
    render_stat(
        frame,
        chunks[2],
        "Not Ready",
        counts.not_ready,
        theme::AMBER,
    );
    render_stat(frame, chunks[3], "Running", counts.running, theme::BLUE);
    render_stat(frame, chunks[4], "Discarded", counts.discarded, theme::RED);
    render_stat(
        frame,
        chunks[5],
        "Committed",
        counts.committed,
        theme::GREEN,
    );
    render_stat(frame, chunks[6], "Pushed", counts.pushed, theme::CYAN);
    render_stat(frame, chunks[7], "PR", counts.pr, theme::CYAN);
}

fn render_compact_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let counts = app.counts();
    let line = Line::from(vec![
        Span::styled("Keel", theme::title()),
        Span::raw("  "),
        Span::styled("Ready ", Style::default().fg(theme::MUTED)),
        Span::styled(counts.ready.to_string(), Style::default().fg(theme::GREEN)),
        Span::raw("  "),
        Span::styled("NotReady ", Style::default().fg(theme::MUTED)),
        Span::styled(
            counts.not_ready.to_string(),
            Style::default().fg(theme::AMBER),
        ),
        Span::raw("  "),
        Span::styled("Running ", Style::default().fg(theme::MUTED)),
        Span::styled(counts.running.to_string(), Style::default().fg(theme::BLUE)),
        Span::raw("  "),
        Span::styled("Git ", Style::default().fg(theme::MUTED)),
        Span::styled(
            format!("c{} p{} pr{}", counts.committed, counts.pushed, counts.pr),
            Style::default().fg(theme::CYAN),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(theme::border()),
            )
            .style(Style::default().bg(theme::BG)),
        area,
    );
}

fn render_stat(frame: &mut Frame<'_>, area: Rect, label: &str, value: usize, color: Color) {
    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(label, Style::default().fg(theme::MUTED))),
        Line::from(Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(theme::border()),
    )
    .style(Style::default().bg(theme::BG));
    frame.render_widget(paragraph, area);
}

fn render_body(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    if area.width < NARROW_WIDTH {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(12)])
            .split(area);

        render_runs(frame, app, chunks[0]);
        render_detail(frame, app, chunks[1]);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(area);

    render_runs(frame, app, chunks[0]);
    render_detail(frame, app, chunks[1]);
}

fn render_runs(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let visible_rows = usize::from(area.height.saturating_sub(3)).max(1);
    app.set_run_list_viewport(visible_rows);
    let rows = app
        .visible_run_window(visible_rows)
        .iter()
        .map(|(index, run)| {
            let selected = *index == app.selected_index();
            let style = if selected {
                theme::selected()
            } else {
                status_row_style(&run.status)
            };
            Row::new(vec![
                Cell::from(short_id(&run.run_id)),
                Cell::from(status_short_label(&run.status)),
                Cell::from(truncate(&run.agent, 8)),
                Cell::from(truncate(&run.task, 34)),
                Cell::from(git_state(run)),
            ])
            .style(style)
        })
        .collect::<Vec<_>>();

    let table = Table::new(rows, RUN_TABLE_WIDTHS)
        .header(
            Row::new(vec!["Run", "State", "Agent", "Task", "Git"])
                .style(Style::default().fg(theme::MUTED).bg(theme::PANEL)),
        )
        .block(
            Block::default()
                .title(run_list_title(app))
                .borders(Borders::ALL)
                .border_style(theme::border())
                .style(Style::default().bg(theme::PANEL)),
        )
        .row_highlight_style(theme::selected());
    frame.render_widget(table, area);

    if app.visible_count() == 0 {
        let message = if app.total_count() == 0 {
            "No runs found. Run `keel run \"task\" --agent noop` first.".to_string()
        } else {
            format!(
                "No runs match filter: {}",
                app.active_filter_label()
                    .unwrap_or_else(|| "<unknown>".to_string())
            )
        };
        render_empty(frame, area, &message);
    }
}

fn render_detail(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let Some(run) = app.selected_run() else {
        render_empty(frame, area, "No selected run.");
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Min(8),
        ])
        .split(area);

    render_run_header(frame, run, chunks[0]);
    render_tabs(frame, app.tab(), chunks[1]);
    render_tab_body(frame, app, chunks[2]);
}

fn render_run_header(frame: &mut Frame<'_>, run: &RunMetadata, area: Rect) {
    let status_color = status_color(&run.status);
    let lines = if area.width < NARROW_WIDTH {
        vec![
            Line::from(vec![
                Span::styled(short_id(&run.run_id), Style::default().fg(theme::CYAN)),
                Span::raw("  "),
                Span::styled(status_label(&run.status), Style::default().fg(status_color)),
                Span::raw("  "),
                Span::styled(truncate(&run.task, 42), Style::default().fg(theme::TEXT)),
            ]),
            Line::from(vec![
                Span::styled("Failure: ", theme::muted()),
                Span::raw(failure_label(run)),
            ]),
            Line::from(vec![
                Span::styled("Readiness: ", theme::muted()),
                Span::raw(compact_reason(&run.readiness_reason)),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled(short_id(&run.run_id), Style::default().fg(theme::CYAN)),
                Span::raw("  "),
                Span::styled(truncate(&run.task, 80), Style::default().fg(theme::TEXT)),
                Span::raw("  "),
                Span::styled(status_label(&run.status), Style::default().fg(status_color)),
            ]),
            Line::from(vec![
                Span::styled("Branch: ", theme::muted()),
                Span::raw(compact_branch(&run.branch)),
                Span::styled("  Worktree: ", theme::muted()),
                Span::raw(compact_path(&run.worktree_path)),
            ]),
            Line::from(vec![
                Span::styled("Failure: ", theme::muted()),
                Span::raw(failure_label(run)),
                Span::styled("  Readiness: ", theme::muted()),
                Span::raw(compact_reason(&run.readiness_reason)),
            ]),
        ]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme::border()),
            )
            .style(Style::default().fg(theme::TEXT).bg(theme::PANEL)),
        area,
    );
}

fn render_tabs(frame: &mut Frame<'_>, active: DetailTab, area: Rect) {
    let tabs = [
        DetailTab::Report,
        DetailTab::Diff,
        DetailTab::Log,
        DetailTab::Artifacts,
    ];
    let selected = tabs.iter().position(|tab| *tab == active).unwrap_or(0);
    let labels = tabs
        .iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(labels)
            .select(selected)
            .highlight_style(
                Style::default()
                    .fg(theme::CYAN)
                    .bg(theme::PANEL)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().fg(theme::MUTED).bg(theme::PANEL))
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(theme::border()),
            ),
        area,
    );
}

fn render_tab_body(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let Some(detail) = app.detail().cloned() else {
        render_empty(
            frame,
            area,
            app.message()
                .unwrap_or("Unable to load selected run artifacts."),
        );
        return;
    };

    match app.tab() {
        DetailTab::Report => render_report(frame, app, &detail, area),
        DetailTab::Diff => render_diff(frame, app, &detail, area),
        DetailTab::Log => render_log(frame, app, &detail, area),
        DetailTab::Artifacts => render_artifacts(frame, app, &detail, area),
    }
}

fn render_report(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let metadata = &detail.report.metadata;
    if area.height < 18 {
        render_compact_report(frame, app, detail, area);
        return;
    }
    app.set_scroll_limit(0, area.height.saturating_sub(2));

    let constraints = if area.height < 24 {
        vec![
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Min(3),
        ]
    } else {
        vec![
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Min(5),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let summary = vec![
        Line::from(vec![label("Run ID"), Span::raw(metadata.run_id.clone())]),
        Line::from(vec![
            label("Status"),
            Span::raw(metadata.status.to_string()),
        ]),
        Line::from(vec![
            label("Failure"),
            Span::raw(
                metadata
                    .failure_reason
                    .as_ref()
                    .map_or_else(|| "none".to_string(), ToString::to_string),
            ),
        ]),
        Line::from(vec![
            label("Ready"),
            Span::raw(compact_reason(&metadata.readiness_reason)),
        ]),
        Line::from(vec![label("Agent"), Span::raw(metadata.agent.clone())]),
        Line::from(vec![
            label("Branch"),
            Span::raw(compact_branch(&metadata.branch)),
        ]),
        Line::from(vec![
            label("Base"),
            Span::raw(short_commit(&metadata.base_commit)),
        ]),
        Line::from(vec![
            label("Created"),
            Span::raw(metadata.created_at.clone()),
        ]),
    ];
    frame.render_widget(section("Run Metadata", summary), chunks[0]);

    let checks = detail
        .checks
        .as_deref()
        .map(check_lines)
        .unwrap_or_else(|| vec![Line::from("checks.json missing")]);
    frame.render_widget(section("Checks", checks), chunks[1]);

    let warnings = warning_lines(metadata);
    frame.render_widget(section("Risk Warnings", warnings), chunks[2]);

    let next = next_action_lines(metadata);
    frame.render_widget(section("Suggested Next Actions", next), chunks[3]);
}

fn render_compact_report(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let metadata = &detail.report.metadata;
    let mut lines = vec![
        Line::from(vec![label("Run ID"), Span::raw(metadata.run_id.clone())]),
        Line::from(vec![
            label("Status"),
            Span::styled(
                metadata.status.to_string(),
                Style::default().fg(status_color(&metadata.status)),
            ),
        ]),
        Line::from(vec![label("Failure"), Span::raw(failure_label(metadata))]),
        Line::from(vec![
            label("Ready"),
            Span::raw(compact_reason(&metadata.readiness_reason)),
        ]),
    ];

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Checks", theme::muted())));
    lines.extend(
        detail
            .checks
            .as_deref()
            .map(check_lines)
            .unwrap_or_else(|| vec![Line::from("checks.json missing")])
            .into_iter()
            .take(3),
    );

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Warnings", theme::muted())));
    lines.extend(warning_lines(metadata).into_iter().take(2));

    render_lines_panel(frame, area, "Review Summary", lines, app);
}

fn warning_lines(metadata: &RunMetadata) -> Vec<Line<'static>> {
    if metadata.warnings.is_empty() && metadata.risk_warnings.is_empty() {
        vec![Line::from("none")]
    } else {
        let mut lines = metadata
            .warnings
            .iter()
            .map(|warning| {
                Line::from(vec![
                    Span::styled("! ", Style::default().fg(theme::AMBER)),
                    Span::raw(truncate(warning, 78)),
                ])
            })
            .collect::<Vec<_>>();

        lines.extend(metadata.risk_warnings.iter().map(risk_warning_line));
        lines.truncate(5);
        lines
    }
}

fn render_diff(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let (title, lines) = match &detail.diff {
        Some(diff) if diff.is_empty => ("Diff", vec![Line::from("diff.patch is empty")]),
        Some(diff) => ("Diff", text_lines(&diff.content)),
        None => ("Diff", vec![Line::from("diff.patch missing")]),
    };
    render_lines_panel(frame, area, title, lines, app);
}

fn render_log(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let (title, lines) = match &detail.log {
        Some(log) if log.is_empty => ("Log", vec![Line::from("log.txt is empty")]),
        Some(log) => ("Log", text_lines(&log.content)),
        None => ("Log", vec![Line::from("log.txt missing")]),
    };
    render_lines_panel(frame, area, title, lines, app);
}

fn render_artifacts(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let artifacts = detail.report.artifacts.iter().collect::<Vec<_>>();
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        "Review artifacts",
        theme::muted(),
    )]));
    lines.extend(
        artifacts
            .iter()
            .filter(|artifact| is_required_artifact(artifact.label))
            .map(|artifact| artifact_line(artifact)),
    );

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Git artifacts",
        theme::muted(),
    )]));
    lines.extend(
        artifacts
            .iter()
            .filter(|artifact| !is_required_artifact(artifact.label))
            .map(|artifact| artifact_line(artifact)),
    );

    render_lines_panel(frame, area, "Artifacts", lines, app);
}

fn artifact_line(artifact: &ArtifactInfo) -> Line<'static> {
    let required = is_required_artifact(artifact.label);
    let (state, color) = match (artifact.exists, required) {
        (true, _) => ("present", theme::GREEN),
        (false, true) => ("missing", theme::RED),
        (false, false) => ("not yet", theme::MUTED),
    };

    Line::from(vec![
        Span::styled(
            format!("{:<10}", artifact.label),
            Style::default().fg(theme::CYAN),
        ),
        Span::styled(format!("{state:<8}"), Style::default().fg(color)),
        Span::raw(compact_path(&artifact.path.display().to_string())),
    ])
}

fn is_required_artifact(label: &str) -> bool {
    matches!(label, "Metadata" | "Log" | "Diff" | "Checks" | "Report")
}

fn render_lines_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    app: &mut App,
) {
    let visible_height = usize::from(area.height.saturating_sub(2));
    let content_len = lines.len();
    app.set_scroll_limit(content_len, area.height.saturating_sub(2));
    let scroll = app.scroll_offset();
    let max_start = content_len.saturating_sub(visible_height);
    let start = usize::from(scroll).min(max_start);
    let mut visible = lines
        .into_iter()
        .skip(start)
        .take(visible_height.max(1))
        .collect::<Vec<_>>();

    if start > 0 {
        visible.insert(
            0,
            Line::from(Span::styled(
                format!("... {start} lines above ..."),
                theme::muted(),
            )),
        );
    }
    if start + visible_height < content_len {
        visible.push(Line::from(Span::styled(
            "... more below; use PgUp/PgDn ...",
            theme::muted(),
        )));
    }

    frame.render_widget(
        section(
            &scroll_title(title, start, visible_height, content_len),
            visible,
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let status = app.message().unwrap_or("read-only review mode");
    let filter = app
        .active_filter_label()
        .map(|label| format!("filter: {label}"))
        .unwrap_or_else(|| "filter: <none>".to_string());
    let mode = if app.filter_mode() {
        "FILTER"
    } else {
        "REVIEW"
    };
    if area.width < 180 {
        let line = Line::from(vec![
            Span::styled("j/k", key_style()),
            Span::raw(" "),
            Span::styled("Tab", key_style()),
            Span::raw(" "),
            Span::styled("/", key_style()),
            Span::raw(" "),
            Span::styled("?", key_style()),
            Span::raw(" "),
            Span::styled("PgUp/PgDn", key_style()),
            Span::raw(" "),
            Span::styled("r", key_style()),
            Span::raw(" "),
            Span::styled("q", key_style()),
            Span::raw("  "),
            Span::styled(mode, Style::default().fg(theme::AMBER)),
            Span::raw("  "),
            Span::styled(truncate(&filter, 20), Style::default().fg(theme::MUTED)),
        ]);
        render_footer_line(frame, area, line);
        return;
    }

    let line = Line::from(vec![
        Span::styled("j/k", key_style()),
        Span::raw(" move  "),
        Span::styled("Tab", key_style()),
        Span::raw(" next tab  "),
        Span::styled("Shift+Tab", key_style()),
        Span::raw(" prev tab  "),
        Span::styled("r", key_style()),
        Span::raw(" refresh  "),
        Span::styled("/", key_style()),
        Span::raw(" filter  "),
        Span::styled("?", key_style()),
        Span::raw(" help  "),
        Span::styled("PgUp/PgDn", key_style()),
        Span::raw(" detail scroll  "),
        Span::styled("q", key_style()),
        Span::raw(" quit  "),
        Span::styled(mode, Style::default().fg(theme::AMBER)),
        Span::raw("  "),
        Span::styled(filter, Style::default().fg(theme::MUTED)),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(theme::MUTED)),
    ]);
    render_footer_line(frame, area, line);
}

fn render_footer_line(frame: &mut Frame<'_>, area: Rect, line: Line<'_>) {
    frame.render_widget(
        Paragraph::new(line)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(theme::border()),
            )
            .style(Style::default().bg(theme::BG)),
        area,
    );
}

fn render_help_overlay(frame: &mut Frame<'_>, area: Rect) {
    let overlay = centered_rect(area, 92, 24);
    let lines = help_overlay_lines();

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Help")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::CYAN).bg(theme::PANEL)),
            )
            .style(Style::default().fg(theme::TEXT).bg(theme::PANEL))
            .wrap(Wrap { trim: false }),
        overlay,
    );
}

fn help_overlay_lines() -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled("Keel TUI Help", theme::title())),
        Line::from(""),
    ];
    lines.extend(help_section(
        "Navigation",
        [
            ("j / Down", "select next run"),
            ("k / Up", "select previous run"),
            ("Tab", "next detail tab"),
            ("Shift+Tab", "previous detail tab"),
            ("PgUp / PgDn", "scroll the current detail tab"),
            (
                "Home / End",
                "jump to top or bottom of the current detail tab",
            ),
        ],
    ));
    lines.push(Line::from(""));
    lines.extend(help_section(
        "Review",
        [
            (
                "/",
                "filter runs by id, task, status, agent, warning, or git state",
            ),
            ("r", "refresh run list and selected artifacts"),
            ("?", "show or hide this help"),
        ],
    ));
    lines.extend([
        Line::from(""),
        Line::from(vec![Span::styled("Safety Boundary", theme::muted())]),
        Line::from(
            "Read-only: never commits, pushes, creates PRs, discards runs, merges, or edits artifacts.",
        ),
        Line::from(
            "Use CLI write commands only after reviewing report, diff, log, checks, and warnings.",
        ),
        Line::from("Human review remains the final merge decision."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Close", key_style()),
            Span::raw("  Esc / Enter / ?    "),
            Span::styled("Quit", key_style()),
            Span::raw("  q"),
        ]),
    ]);
    lines
}

fn help_section(
    title: &'static str,
    items: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> Vec<Line<'static>> {
    std::iter::once(Line::from(vec![Span::styled(title, theme::muted())]))
        .chain(
            items
                .into_iter()
                .map(|(key, description)| help_line(key, description)),
        )
        .collect()
}

fn centered_rect(area: Rect, max_width: u16, max_height: u16) -> Rect {
    let width = max_width.min(area.width.saturating_sub(4)).max(20);
    let height = max_height.min(area.height.saturating_sub(4)).max(10);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn help_line(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<14}"), key_style()),
        Span::raw(description),
    ])
}

fn key_style() -> Style {
    Style::default()
        .fg(theme::CYAN)
        .add_modifier(Modifier::BOLD)
}

fn run_list_title(app: &App) -> String {
    let position = app
        .selected_position()
        .map(|(selected, total)| format!("{selected}/{total}"))
        .unwrap_or_else(|| "0/0".to_string());
    if !app.has_active_filters() {
        format!("Runs ({position}, newest first)")
    } else {
        format!(
            "Runs ({position}, {} of {}, filter: {})",
            app.visible_count(),
            app.total_count(),
            app.active_filter_label().unwrap_or_default()
        )
    }
}

fn scroll_title(title: &str, start: usize, visible_height: usize, content_len: usize) -> String {
    if content_len <= visible_height || content_len == 0 {
        title.to_string()
    } else {
        let first = start + 1;
        let last = (start + visible_height).min(content_len);
        format!("{title} ({first}-{last}/{content_len})")
    }
}

fn render_empty(frame: &mut Frame<'_>, area: Rect, message: &str) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme::border()),
            )
            .style(Style::default().fg(theme::MUTED).bg(theme::PANEL)),
        area,
    );
}

fn section<'a>(title: &'a str, lines: Vec<Line<'a>>) -> Paragraph<'a> {
    Paragraph::new(lines)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(theme::border()),
        )
        .style(Style::default().fg(theme::TEXT).bg(theme::PANEL))
        .wrap(Wrap { trim: false })
}

fn label(value: &'static str) -> Span<'static> {
    Span::styled(format!("{value:<10} "), Style::default().fg(theme::MUTED))
}

fn check_lines(checks: &[CheckResult]) -> Vec<Line<'static>> {
    if checks.is_empty() {
        return vec![Line::from("no checks recorded")];
    }

    sorted_checks_for_review(checks)
        .into_iter()
        .take(7)
        .map(|check| {
            let color = match check.status {
                CheckStatus::Passed => theme::GREEN,
                CheckStatus::Failed => theme::RED,
                CheckStatus::Skipped => theme::MUTED,
            };
            let exit = check
                .exit_code
                .map_or_else(|| "-".to_string(), |code| code.to_string());
            Line::from(vec![
                Span::styled(check_marker(&check.status), Style::default().fg(color)),
                Span::raw(truncate(&check.name, 30)),
                Span::raw("  "),
                Span::styled(check.status.to_string(), Style::default().fg(color)),
                Span::raw("  exit "),
                Span::styled(exit, Style::default().fg(color)),
            ])
        })
        .collect()
}

fn sorted_checks_for_review(checks: &[CheckResult]) -> Vec<&CheckResult> {
    let mut checks = checks.iter().collect::<Vec<_>>();
    checks.sort_by_key(|check| match check.status {
        CheckStatus::Failed => 0,
        CheckStatus::Skipped => 1,
        CheckStatus::Passed => 2,
    });
    checks
}

fn check_marker(status: &CheckStatus) -> &'static str {
    match status {
        CheckStatus::Passed => "✓ ",
        CheckStatus::Failed => "✗ ",
        CheckStatus::Skipped => "- ",
    }
}

fn next_action_lines(metadata: &RunMetadata) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Review report, diff, log, checks, and warnings before merging."),
        Line::from("Human decides merge. Keel does not auto merge."),
    ];
    if metadata.committed {
        lines.push(Line::from("Local commit exists on the candidate branch."));
    } else if metadata.status == RunStatus::Ready {
        lines.push(Line::from(format!("CLI: keel commit {}", metadata.run_id)));
    }
    if metadata.pushed {
        lines.push(Line::from("Candidate branch has been pushed."));
    } else if metadata.committed && metadata.status == RunStatus::Ready {
        lines.push(Line::from(format!("CLI: keel push {}", metadata.run_id)));
    }
    if metadata.pr_created {
        lines.push(Line::from("PR/MR artifact exists."));
    } else if metadata.pushed && metadata.status == RunStatus::Ready {
        lines.push(Line::from(format!(
            "CLI: keel pr {} --manual --dry-run",
            metadata.run_id
        )));
    }
    lines
}

fn git_state(run: &RunMetadata) -> String {
    if run.pr_created {
        "pr".to_string()
    } else if run.pushed {
        "pushed".to_string()
    } else if run.committed {
        "commit".to_string()
    } else {
        "-".to_string()
    }
}

fn status_row_style(status: &RunStatus) -> Style {
    Style::default().fg(status_color(status)).bg(theme::PANEL)
}

fn risk_warning_line(warning: &RiskWarning) -> Line<'static> {
    let color = match warning.kind {
        RiskWarningKind::RiskPath | RiskWarningKind::HighRiskPath => theme::AMBER,
        RiskWarningKind::DependencyManifest | RiskWarningKind::Lockfile => theme::BLUE,
        RiskWarningKind::DeletedFile | RiskWarningKind::InvalidRiskPattern => theme::RED,
        RiskWarningKind::LargeDiff => theme::AMBER,
    };
    let mut text = warning.message.clone();
    if let Some(details) = &warning.details {
        text.push_str(": ");
        text.push_str(details);
    }
    Line::from(vec![
        Span::styled("! ", Style::default().fg(color)),
        Span::styled(warning.kind.to_string(), Style::default().fg(color)),
        Span::raw("  "),
        Span::raw(truncate(&text, 72)),
    ])
}

fn failure_label(run: &RunMetadata) -> String {
    run.failure_reason
        .as_ref()
        .map_or_else(|| "none".to_string(), ToString::to_string)
}

fn status_label(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Created => "created",
        RunStatus::Running => "running",
        RunStatus::Ready => "ready",
        RunStatus::NotReady => "not_ready",
        RunStatus::Discarded => "discarded",
    }
}

fn status_short_label(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Created => "created",
        RunStatus::Running => "running",
        RunStatus::Ready => "ready",
        RunStatus::NotReady => "notready",
        RunStatus::Discarded => "discard",
    }
}

fn status_color(status: &RunStatus) -> Color {
    match status {
        RunStatus::Created => theme::BLUE,
        RunStatus::Running => theme::BLUE,
        RunStatus::Ready => theme::GREEN,
        RunStatus::NotReady => theme::AMBER,
        RunStatus::Discarded => theme::RED,
    }
}

fn short_id(value: &str) -> String {
    truncate(value, 12)
}

fn short_commit(value: &str) -> String {
    truncate(value, 12)
}

fn compact_branch(value: &str) -> String {
    compact_path_like(value)
}

fn compact_path(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let relative = normalized
        .find(".keel/")
        .map_or(normalized.as_str(), |index| &normalized[index..]);
    compact_path_like(relative)
}

fn compact_path_like(value: &str) -> String {
    value
        .split('/')
        .map(compact_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn compact_path_segment(segment: &str) -> String {
    if segment.starts_with("run-") {
        short_id(segment)
    } else {
        segment.to_string()
    }
}

fn compact_reason(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "n/a".to_string()
    } else {
        truncate(value, 72)
    }
}

fn text_lines(value: &str) -> Vec<Line<'static>> {
    let lines = value
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec![Line::from("(empty)")]
    } else {
        lines
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut out = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_state_prefers_pr_over_push_over_commit() {
        let mut run = sample_run();

        assert_eq!(git_state(&run), "-");
        run.committed = true;
        assert_eq!(git_state(&run), "commit");
        run.pushed = true;
        assert_eq!(git_state(&run), "pushed");
        run.pr_created = true;
        assert_eq!(git_state(&run), "pr");
    }

    #[test]
    fn check_lines_prioritize_failed_checks_and_show_exit_code() {
        let checks = vec![
            check("git status", CheckStatus::Passed, Some(0)),
            check("cargo test", CheckStatus::Failed, Some(101)),
        ];

        let lines = check_lines(&checks);
        let rendered = format!("{:?}", lines[0]);

        assert!(rendered.contains("cargo test"));
        assert!(rendered.contains("failed"));
        assert!(rendered.contains("101"));
    }

    #[test]
    fn failure_label_uses_metadata_failure_reason() {
        let mut run = sample_run();

        assert_eq!(failure_label(&run), "none");

        run.failure_reason = Some(keel_core::FailureReason::CheckFailed);
        assert_eq!(failure_label(&run), "check_failed");
    }

    #[test]
    fn compact_paths_keep_keel_relative_context() {
        let path = r"C:\tmp\repo\.keel\runs\run-1777642077378-46484\metadata.json";

        assert_eq!(compact_path(path), ".keel/runs/run-17776420…/metadata.json");
        assert_eq!(
            compact_branch("keel/run/run-1777642077378-46484"),
            "keel/run/run-17776420…"
        );
    }

    fn check(name: &str, status: CheckStatus, exit_code: Option<i32>) -> CheckResult {
        CheckResult {
            name: name.to_string(),
            command: name.to_string(),
            status,
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn sample_run() -> RunMetadata {
        RunMetadata {
            run_id: "run-1".to_string(),
            parent_run_id: None,
            task: "task".to_string(),
            agent: "noop".to_string(),
            status: RunStatus::Ready,
            created_at: "2026-05-01T00:00:00Z".to_string(),
            updated_at: "2026-05-01T00:00:00Z".to_string(),
            started_at: None,
            finished_at: None,
            duration_ms: None,
            worktree_path: ".keel/worktrees/run-1".to_string(),
            run_dir: ".keel/runs/run-1".to_string(),
            branch: "keel/run/run-1".to_string(),
            base_commit: "base".to_string(),
            agent_command: Vec::new(),
            exit_code: None,
            failure_reason: None,
            readiness_reason: String::new(),
            warnings: Vec::new(),
            risk_warnings: Vec::new(),
            committed: false,
            commit_sha: None,
            commit_message: None,
            committed_at: None,
            commit: None,
            pushed: false,
            pushed_at: None,
            push_remote: None,
            push_remote_url: None,
            pushed_branch: None,
            push: None,
            pr_created: false,
            pr_created_at: None,
            pr_provider: None,
            pr_url: None,
            pr_target_branch: None,
            pr_source_branch: None,
            pr: None,
        }
    }
}
