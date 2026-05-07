use crate::app::{App, DetailTab};
use crate::theme;
use keel_core::{
    artifact_files, artifact_keys, primary_next_action, ArtifactInfo, CheckResult, CheckStatus,
    ReviewNextActionKind, RiskWarning, RiskWarningKind, RunArtifacts, RunMetadata, RunStatus,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs, Wrap};
use ratatui::Frame;

const NARROW_WIDTH: u16 = 110;

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let root = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(theme::BG)), root);
    let header_height = 2;

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

    let title = Paragraph::new(Line::from(vec![Span::styled(
        "Keel review",
        theme::title(),
    )]))
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
        Span::styled("Keel review", theme::title()),
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
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(label, Style::default().fg(theme::MUTED)),
        Span::raw(" "),
        Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ]))
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
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
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
                Cell::from(short_run_id_for_width(&run.run_id, 14)),
                Cell::from(truncate(&run.agent, 8)),
                Cell::from(truncate(&review_state_label(run), 14)),
            ])
            .style(style)
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Length(8),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["Run", "Agent", "State"])
            .style(Style::default().fg(theme::MUTED).bg(theme::PANEL)),
    )
    .block(
        Block::default()
            .title(review_queue_title(app))
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
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(8),
        ])
        .split(area);

    render_run_header(frame, run, chunks[0]);
    render_tabs(frame, app.tab(), app.detail(), chunks[1]);
    render_tab_body(frame, app, chunks[2]);
}

fn render_run_header(frame: &mut Frame<'_>, run: &RunMetadata, area: Rect) {
    let lines = if area.width < NARROW_WIDTH {
        vec![
            Line::from(vec![
                Span::styled(short_run_id(&run.run_id), Style::default().fg(theme::CYAN)),
                Span::raw("  "),
                Span::styled(review_state_label(run), decision_style(run)),
                Span::raw("  "),
                Span::raw(truncate(&run.task, 48)),
            ]),
            Line::from(vec![
                Span::styled("Action: ", theme::muted()),
                Span::styled(next_step_text(run), next_action_style(run)),
                Span::styled("  Agent ", theme::muted()),
                Span::raw(run.agent.clone()),
            ]),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled(short_run_id(&run.run_id), Style::default().fg(theme::CYAN)),
                Span::raw("  "),
                Span::styled(review_state_label(run), decision_style(run)),
                Span::raw("  "),
                Span::raw(truncate(&run.task, 80)),
            ]),
            Line::from(vec![
                Span::styled("Action: ", theme::muted()),
                Span::styled(next_step_text(run), next_action_style(run)),
                Span::styled("  Agent ", theme::muted()),
                Span::raw(run.agent.clone()),
                Span::styled("  Git ", theme::muted()),
                Span::raw(git_state(run)),
                Span::styled("  Branch ", theme::muted()),
                Span::raw(compact_branch(&run.branch)),
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

fn render_tabs(
    frame: &mut Frame<'_>,
    active: DetailTab,
    detail: Option<&RunArtifacts>,
    area: Rect,
) {
    let tabs = [
        DetailTab::Report,
        DetailTab::Diff,
        DetailTab::Log,
        DetailTab::Artifacts,
    ];
    let selected = tabs.iter().position(|tab| *tab == active).unwrap_or(0);
    let labels = tabs
        .iter()
        .map(|tab| tab_label(*tab, detail))
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

fn tab_label(tab: DetailTab, detail: Option<&RunArtifacts>) -> Line<'static> {
    let Some((marker, color)) = detail.and_then(|detail| tab_state_marker(tab, detail)) else {
        return Line::from(tab.title());
    };

    Line::from(vec![
        Span::raw(tab.title()),
        Span::raw(" "),
        Span::styled(
            marker,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn tab_state_marker(tab: DetailTab, detail: &RunArtifacts) -> Option<(&'static str, Color)> {
    match tab {
        DetailTab::Report if !artifact_exists(detail, artifact_keys::REPORT) => {
            Some(("missing", theme::RED))
        }
        DetailTab::Report => None,
        DetailTab::Diff => match &detail.diff {
            Some(diff) if diff.is_empty => Some(("empty", theme::AMBER)),
            Some(_) => Some(("+", theme::GREEN)),
            None => Some(("missing", theme::RED)),
        },
        DetailTab::Log => match &detail.log {
            Some(log) if log.is_empty => Some(("empty", theme::AMBER)),
            Some(_) => Some(("+", theme::GREEN)),
            None => Some(("missing", theme::RED)),
        },
        DetailTab::Artifacts if has_missing_required_artifact(detail) => Some(("!", theme::RED)),
        DetailTab::Artifacts => None,
    }
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
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(3),
        ]
    } else {
        vec![
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(3),
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    frame.render_widget(
        section(
            "Current Decision",
            review_focus_lines(metadata, detail.checks.as_deref()),
        ),
        chunks[0],
    );

    frame.render_widget(
        section("Next CLI Step", review_progress_lines(metadata)),
        chunks[1],
    );

    let checks = detail
        .checks
        .as_deref()
        .map(check_lines)
        .unwrap_or_else(missing_checks_lines);
    frame.render_widget(section("Checks", checks), chunks[2]);

    let warnings = warning_lines(metadata);
    frame.render_widget(section("Risk Warnings", warnings), chunks[3]);
}

fn render_compact_report(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let metadata = &detail.report.metadata;
    let mut lines = review_focus_lines(metadata, detail.checks.as_deref());

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Progress", theme::muted())));
    lines.extend(compact_review_progress_lines(metadata));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Checks", theme::muted())));
    lines.extend(
        detail
            .checks
            .as_deref()
            .map(check_lines)
            .unwrap_or_else(missing_checks_lines)
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

fn review_focus_lines(
    metadata: &RunMetadata,
    checks: Option<&[CheckResult]>,
) -> Vec<Line<'static>> {
    let warning_count = metadata.warnings.len() + metadata.risk_warnings.len();
    let failed_checks = checks
        .unwrap_or_default()
        .iter()
        .filter(|check| check.status == CheckStatus::Failed)
        .count();
    vec![
        Line::from(vec![
            Span::styled(
                status_label(&metadata.status),
                Style::default()
                    .fg(status_color(&metadata.status))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                review_verdict(metadata, failed_checks),
                verdict_style(metadata),
            ),
        ]),
        Line::from(vec![label("Task"), Span::raw(truncate(&metadata.task, 82))]),
        Line::from(vec![
            label("Evidence"),
            Span::styled(
                risk_summary(warning_count),
                Style::default().fg(if warning_count == 0 {
                    theme::GREEN
                } else {
                    theme::AMBER
                }),
            ),
            Span::styled("  Checks ", theme::muted()),
            Span::styled(
                check_summary(checks, failed_checks),
                Style::default().fg(if failed_checks == 0 {
                    theme::GREEN
                } else {
                    theme::RED
                }),
            ),
        ]),
        Line::from(vec![
            label("Agent"),
            Span::raw(metadata.agent.clone()),
            Span::styled("  Branch ", theme::muted()),
            Span::raw(compact_branch(&metadata.branch)),
        ]),
        Line::from(vec![
            label("Ready"),
            Span::raw(compact_reason(&metadata.readiness_reason)),
        ]),
    ]
}

fn review_verdict(metadata: &RunMetadata, failed_checks: usize) -> &'static str {
    match metadata.status {
        RunStatus::Ready if failed_checks == 0 => "ready for human review",
        RunStatus::Ready => "ready, but checks need attention",
        RunStatus::NotReady => "not ready: inspect checks and logs",
        RunStatus::Discarded => "discarded candidate",
        RunStatus::Running => "agent still running",
        RunStatus::Created => "run created, waiting for execution",
    }
}

fn verdict_style(metadata: &RunMetadata) -> Style {
    Style::default().fg(match metadata.status {
        RunStatus::Ready => theme::GREEN,
        RunStatus::NotReady => theme::AMBER,
        RunStatus::Discarded => theme::RED,
        RunStatus::Running | RunStatus::Created => theme::BLUE,
    })
}

fn risk_summary(count: usize) -> String {
    match count {
        0 => "none".to_string(),
        1 => "1 warning".to_string(),
        count => format!("{count} warnings"),
    }
}

fn check_summary(checks: Option<&[CheckResult]>, failed_checks: usize) -> String {
    let Some(checks) = checks else {
        return "missing".to_string();
    };
    if checks.is_empty() {
        return "none recorded".to_string();
    }
    if failed_checks == 0 {
        format!("{} passed", checks.len())
    } else {
        format!("{failed_checks}/{} failed", checks.len())
    }
}

fn render_diff(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let (title, lines) = match &detail.diff {
        Some(diff) if diff.is_empty => ("Diff", empty_artifact_lines(artifact_files::DIFF)),
        Some(diff) => ("Diff Review", diff_lines(&diff.content)),
        None => ("Diff", missing_artifact_lines(artifact_files::DIFF)),
    };
    render_lines_panel(frame, area, title, lines, app);
}

fn render_log(frame: &mut Frame<'_>, app: &mut App, detail: &RunArtifacts, area: Rect) {
    let (title, lines) = match &detail.log {
        Some(log) if log.is_empty => ("Log", empty_artifact_lines(artifact_files::LOG)),
        Some(log) => ("Log", text_lines(&log.content)),
        None => ("Log", missing_artifact_lines(artifact_files::LOG)),
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
            .filter(|artifact| artifact.required)
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
            .filter(|artifact| !artifact.required)
            .map(|artifact| artifact_line(artifact)),
    );

    render_lines_panel(frame, area, "Artifacts", lines, app);
}

fn artifact_line(artifact: &ArtifactInfo) -> Line<'static> {
    let (state, color) = match (artifact.exists, artifact.required) {
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

fn artifact_exists(detail: &RunArtifacts, key: &str) -> bool {
    detail
        .report
        .artifacts
        .iter()
        .any(|artifact| artifact.key == key && artifact.exists)
}

fn has_missing_required_artifact(detail: &RunArtifacts) -> bool {
    detail
        .report
        .artifacts
        .iter()
        .any(|artifact| artifact.required && !artifact.exists)
}

fn missing_artifact_lines(file: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("missing ", Style::default().fg(theme::RED)),
            Span::raw(file),
        ]),
        Line::from("This run artifact was not found in .keel/runs/<run-id>/."),
        Line::from("Use the Artifacts tab to inspect which files are present."),
    ]
}

fn empty_artifact_lines(file: &'static str) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("empty ", Style::default().fg(theme::AMBER)),
            Span::raw(file),
        ]),
        Line::from("The artifact exists but has no reviewable content."),
    ]
}

fn missing_checks_lines() -> Vec<Line<'static>> {
    missing_artifact_lines(artifact_files::CHECKS)
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
    let line = if area.width < 96 {
        Line::from(vec![
            Span::styled("Move ", theme::muted()),
            Span::styled("j/k", key_style()),
            Span::raw("  "),
            Span::styled("Tabs ", theme::muted()),
            Span::styled("1-4", key_style()),
            Span::raw("  "),
            Span::styled("Scroll ", theme::muted()),
            Span::styled("PgUp/PgDn", key_style()),
            Span::raw("  "),
            Span::styled("Filter ", theme::muted()),
            Span::styled("/", key_style()),
            Span::raw("  "),
            Span::styled("Help ", theme::muted()),
            Span::styled("?", key_style()),
            Span::raw("  "),
            Span::styled("Quit ", theme::muted()),
            Span::styled("q", key_style()),
            Span::raw("  "),
            Span::styled(mode, Style::default().fg(theme::AMBER)),
        ])
    } else if area.width < 180 {
        Line::from(vec![
            Span::styled("Move ", theme::muted()),
            Span::styled("j/k", key_style()),
            Span::raw(" "),
            Span::styled("g/G", key_style()),
            Span::raw("  "),
            Span::styled("Tabs ", theme::muted()),
            Span::styled("1-4", key_style()),
            Span::raw(" "),
            Span::styled("Tab", key_style()),
            Span::raw("  "),
            Span::styled("Filter ", theme::muted()),
            Span::styled("/", key_style()),
            Span::raw("  "),
            Span::styled("Help ", theme::muted()),
            Span::styled("?", key_style()),
            Span::raw("  "),
            Span::styled("Scroll ", theme::muted()),
            Span::styled("PgUp/PgDn", key_style()),
            Span::raw("  "),
            Span::styled("Refresh ", theme::muted()),
            Span::styled("r", key_style()),
            Span::raw("  "),
            Span::styled("Quit ", theme::muted()),
            Span::styled("q", key_style()),
            Span::raw("  "),
            Span::styled(mode, Style::default().fg(theme::AMBER)),
            Span::raw("  "),
            Span::styled(truncate(&filter, 24), Style::default().fg(theme::MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled("Move ", theme::muted()),
            Span::styled("j/k", key_style()),
            Span::raw(" next/prev  "),
            Span::styled("g/G", key_style()),
            Span::raw(" first/last  "),
            Span::styled("Tabs ", theme::muted()),
            Span::styled("1-4", key_style()),
            Span::raw(" direct  "),
            Span::styled("Tab", key_style()),
            Span::raw(" next  "),
            Span::styled("Shift+Tab", key_style()),
            Span::raw(" prev  "),
            Span::styled("Filter ", theme::muted()),
            Span::styled("/", key_style()),
            Span::raw("  "),
            Span::styled("Help ", theme::muted()),
            Span::styled("?", key_style()),
            Span::raw("  "),
            Span::styled("Scroll ", theme::muted()),
            Span::styled("PgUp/PgDn", key_style()),
            Span::raw(" detail  "),
            Span::styled("Refresh ", theme::muted()),
            Span::styled("r", key_style()),
            Span::raw("  "),
            Span::styled("Quit ", theme::muted()),
            Span::styled("q", key_style()),
            Span::raw("  "),
            Span::styled(mode, Style::default().fg(theme::AMBER)),
            Span::raw("  "),
            Span::styled(filter, Style::default().fg(theme::MUTED)),
            Span::raw("  "),
            Span::styled(status, Style::default().fg(theme::MUTED)),
        ])
    };
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
    let overlay = centered_rect(area, 112, 30);
    let lines = help_overlay_lines();

    frame.render_widget(Clear, area);
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
            ("g / G", "jump to first or last visible run"),
            ("PgUp / PgDn", "scroll the current detail tab"),
            ("Home / End", "jump to top or bottom of detail"),
        ],
    ));
    lines.push(Line::from(""));
    lines.extend(help_section(
        "Tabs",
        [
            ("1 / 2", "open report or diff"),
            ("3 / 4", "open log or artifacts"),
            ("Tab", "next detail tab"),
            ("Shift+Tab", "previous detail tab"),
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

fn review_queue_title(app: &App) -> String {
    let position = app
        .selected_position()
        .map(|(selected, total)| format!("{selected}/{total}"))
        .unwrap_or_else(|| "0/0".to_string());
    if !app.has_active_filters() {
        format!("Review queue ({position}, newest first)")
    } else {
        format!(
            "Review queue ({position}, {} of {}, filter: {})",
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

fn review_progress_lines(metadata: &RunMetadata) -> Vec<Line<'static>> {
    vec![
        progress_line(
            "Commit",
            metadata.has_commit_record(),
            committed_detail(metadata),
            "not committed",
        ),
        progress_line(
            "Push",
            metadata.has_push_record(),
            pushed_detail(metadata),
            "not pushed",
        ),
        progress_line(
            "PR/MR",
            metadata.has_pr_record(),
            pr_detail(metadata),
            "not created",
        ),
        Line::from(vec![
            label("Next"),
            Span::styled(
                review_next_action_text(metadata),
                next_action_style(metadata),
            ),
        ]),
    ]
}

fn compact_review_progress_lines(metadata: &RunMetadata) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            label("Git"),
            progress_chip(artifact_keys::COMMIT, metadata.has_commit_record()),
            Span::raw("  "),
            progress_chip(artifact_keys::PUSH, metadata.has_push_record()),
            Span::raw("  "),
            progress_chip(artifact_keys::PR, metadata.has_pr_record()),
        ]),
        Line::from(vec![
            label("Next"),
            Span::styled(
                review_next_action_text(metadata),
                next_action_style(metadata),
            ),
        ]),
    ]
}

fn progress_chip(label: &'static str, done: bool) -> Span<'static> {
    let (marker, color) = if done {
        ("yes", theme::GREEN)
    } else {
        ("no", theme::MUTED)
    };
    Span::styled(format!("{label}:{marker}"), Style::default().fg(color))
}

fn progress_line(
    label_text: &'static str,
    done: bool,
    done_detail: String,
    pending_detail: &'static str,
) -> Line<'static> {
    let (state, color, detail) = if done {
        ("yes", theme::GREEN, done_detail)
    } else {
        ("no ", theme::MUTED, pending_detail.to_string())
    };

    Line::from(vec![
        label(label_text),
        Span::styled(
            state,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(detail),
    ])
}

fn committed_detail(metadata: &RunMetadata) -> String {
    metadata
        .recorded_commit_sha()
        .map(short_sha)
        .unwrap_or_else(|| "local commit recorded".to_string())
}

fn pushed_detail(metadata: &RunMetadata) -> String {
    let remote = metadata.recorded_push_remote().unwrap_or("remote");
    let branch = metadata
        .recorded_pushed_branch()
        .unwrap_or(metadata.branch.as_str());
    format!("{remote} {}", compact_branch(branch))
}

fn pr_detail(metadata: &RunMetadata) -> String {
    if let Some(url) = metadata.recorded_pr_url() {
        return compact_path(url);
    }
    metadata
        .recorded_pr_provider()
        .map(|provider| format!("{provider} request created"))
        .unwrap_or_else(|| "request artifact recorded".to_string())
}

fn review_next_action_text(metadata: &RunMetadata) -> String {
    if let Some(action) = primary_next_action(metadata) {
        return match action.kind {
            ReviewNextActionKind::ReviewProvider => action.command.clone(),
            _ => format!("CLI: {}", action.command),
        };
    }

    match metadata.status {
        RunStatus::Ready => "review provider request; Keel will not merge".to_string(),
        RunStatus::NotReady => "fix or rerun; commit/push are blocked".to_string(),
        RunStatus::Discarded => "history only; no write action suggested".to_string(),
        RunStatus::Running => "wait, then refresh with r".to_string(),
        RunStatus::Created => "waiting for run artifacts".to_string(),
    }
}

fn next_action_style(metadata: &RunMetadata) -> Style {
    Style::default().fg(match metadata.status {
        RunStatus::Ready => theme::CYAN,
        RunStatus::NotReady => theme::AMBER,
        RunStatus::Discarded => theme::RED,
        RunStatus::Running | RunStatus::Created => theme::BLUE,
    })
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

fn git_state(run: &RunMetadata) -> String {
    if run.has_pr_record() {
        artifact_keys::PR.to_string()
    } else if run.has_push_record() {
        "pushed".to_string()
    } else if run.has_commit_record() {
        artifact_keys::COMMIT.to_string()
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

fn status_label(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Created => "created",
        RunStatus::Running => "running",
        RunStatus::Ready => "ready",
        RunStatus::NotReady => "not_ready",
        RunStatus::Discarded => "discarded",
    }
}

fn review_state_label(run: &RunMetadata) -> String {
    let risk_count = run.warnings.len() + run.risk_warnings.len();
    let base = match run.status {
        RunStatus::Ready => "review",
        RunStatus::NotReady => "blocked",
        RunStatus::Running => "running",
        RunStatus::Discarded => "discarded",
        RunStatus::Created => "created",
    };

    match (run.status.clone(), risk_count) {
        (RunStatus::Ready, 0) => "review".to_string(),
        (RunStatus::Ready, count) => format!("review risk:{count}"),
        (_, 0) => base.to_string(),
        (_, count) => format!("{base} risk:{count}"),
    }
}

fn next_step_text(run: &RunMetadata) -> String {
    review_next_action_text(run)
}

fn decision_style(run: &RunMetadata) -> Style {
    Style::default()
        .fg(status_color(&run.status))
        .add_modifier(Modifier::BOLD)
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

fn short_run_id(value: &str) -> String {
    short_run_id_for_width(value, 16)
}

fn short_run_id_for_width(value: &str, max_chars: usize) -> String {
    let Some(suffix) = value.rsplit('-').next() else {
        return truncate(value, max_chars);
    };

    let compact = if value.starts_with("run-") && suffix.len() < value.len() {
        format!("run-...{suffix}")
    } else {
        value.to_string()
    };
    truncate(&compact, max_chars)
}

fn short_sha(value: &str) -> String {
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

fn diff_lines(value: &str) -> Vec<Line<'static>> {
    if value.lines().next().is_none() {
        vec![Line::from("(empty)")]
    } else {
        let mut lines = vec![Line::from(vec![
            Span::styled("+ additions", Style::default().fg(theme::GREEN)),
            Span::raw("  "),
            Span::styled("- deletions", Style::default().fg(theme::RED)),
            Span::raw("  "),
            Span::styled("@@ hunks", Style::default().fg(theme::BLUE)),
            Span::raw("  "),
            Span::styled("file headers", Style::default().fg(theme::CYAN)),
        ])];
        lines.push(Line::from(""));
        lines.extend(value.lines().map(diff_line));
        lines
    }
}

fn diff_line(line: &str) -> Line<'static> {
    let style = if line.starts_with("diff --git") {
        Style::default()
            .fg(theme::CYAN)
            .bg(theme::CYAN_BG)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default()
            .fg(theme::BLUE)
            .bg(theme::BLUE_BG)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("+++") || line.starts_with("---") {
        Style::default().fg(theme::MUTED)
    } else if line.starts_with('+') {
        Style::default().fg(theme::GREEN).bg(theme::GREEN_BG)
    } else if line.starts_with('-') {
        Style::default().fg(theme::RED).bg(theme::RED_BG)
    } else if line.starts_with("new file")
        || line.starts_with("deleted file")
        || line.starts_with("rename from")
        || line.starts_with("rename to")
        || line.starts_with("index ")
    {
        Style::default().fg(theme::AMBER).bg(theme::AMBER_BG)
    } else {
        Style::default().fg(theme::TEXT)
    };

    Line::from(Span::styled(line.to_string(), style))
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
    use keel_core::{
        ArtifactInfo, CommitArtifact, DiffInfo, PrArtifact, PrProvider, PushArtifact, ReportInfo,
        RunArtifactSpec, RunArtifacts, RUN_ARTIFACTS,
    };
    use std::path::PathBuf;

    #[test]
    fn git_state_prefers_pr_over_push_over_commit() {
        let mut run = sample_run();

        assert_eq!(git_state(&run), "-");
        run.commit = Some(sample_commit_artifact(&run, "abc123"));
        assert_eq!(git_state(&run), artifact_keys::COMMIT);
        run.push = Some(sample_push_artifact(
            &run,
            "abc123",
            "git@github.com:example/repo.git",
        ));
        assert_eq!(git_state(&run), "pushed");
        run.pr = Some(sample_pr_artifact(
            &run,
            "abc123",
            "git@github.com:example/repo.git",
            "https://github.com/example/repo/pull/1",
        ));
        assert_eq!(git_state(&run), artifact_keys::PR);
    }

    #[test]
    fn review_state_label_keeps_review_signal_short() {
        let mut run = sample_run();

        assert_eq!(review_state_label(&run), "review");

        run.status = RunStatus::NotReady;
        assert_eq!(review_state_label(&run), "blocked");

        run.warnings.push("dependency manifest changed".to_string());
        assert_eq!(review_state_label(&run), "blocked risk:1");
    }

    #[test]
    fn tab_labels_surface_artifact_state() {
        let run = sample_run();
        let mut detail = sample_artifacts(run);

        let present = format!("{:?}", tab_label(DetailTab::Diff, Some(&detail)));
        assert!(present.contains("Diff"));
        assert!(present.contains("\"+\""));

        let diff = detail.diff.as_mut().expect("sample artifacts include diff");
        diff.is_empty = true;
        assert!(format!("{:?}", tab_label(DetailTab::Diff, Some(&detail))).contains("empty"));

        detail.diff = None;
        assert!(format!("{:?}", tab_label(DetailTab::Diff, Some(&detail))).contains("missing"));

        detail.report.artifacts[0].exists = false;
        assert!(format!("{:?}", tab_label(DetailTab::Artifacts, Some(&detail))).contains("!"));
    }

    #[test]
    fn review_progress_surfaces_next_cli_action() {
        let mut run = sample_run();

        let uncommitted = format!("{:?}", review_progress_lines(&run));
        assert!(uncommitted.contains("Commit"));
        assert!(uncommitted.contains("keel commit run-1"));

        run.commit = Some(sample_commit_artifact(&run, "1234567890abcdef"));
        let committed = format!("{:?}", review_progress_lines(&run));
        assert!(committed.contains("1234567890ab"));
        assert!(committed.contains("keel push run-1"));

        run.push = Some(sample_push_artifact(
            &run,
            "1234567890abcdef",
            "git@github.com:example/repo.git",
        ));
        let pushed = format!("{:?}", review_progress_lines(&run));
        assert!(pushed.contains("origin keel/run/run-1"));
        assert!(pushed.contains("keel pr run-1 --provider github --dry-run"));

        run.pr = Some(sample_pr_artifact(
            &run,
            "1234567890abcdef",
            "git@github.com:example/repo.git",
            "https://github.com/example/repo/pull/1",
        ));
        let pr = format!("{:?}", review_progress_lines(&run));
        assert!(pr.contains("github.com/example/repo/pull/1"));
        assert!(pr.contains("review PR/MR on provider"));
    }

    #[test]
    fn artifact_empty_and_missing_messages_are_actionable() {
        let missing = format!("{:?}", missing_artifact_lines(artifact_files::DIFF));
        let empty = format!("{:?}", empty_artifact_lines(artifact_files::LOG));

        assert!(missing.contains("missing"));
        assert!(missing.contains("Artifacts tab"));
        assert!(empty.contains("empty"));
        assert!(empty.contains("no reviewable content"));
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
    fn diff_lines_style_git_diff_semantics() {
        let lines = diff_lines("diff --git a/file b/file\n@@ -1 +1 @@\n-old\n+new\n context");

        let rendered = format!("{:?}", lines);

        assert!(rendered.contains("diff --git"));
        assert!(rendered.contains("+ additions"));
        assert!(rendered.contains("old"));
        assert!(rendered.contains("new"));
        assert!(rendered.contains("Rgb(238, 106, 106)"));
        assert!(rendered.contains("Rgb(119, 214, 140)"));
        assert!(rendered.contains("Rgb(49, 22, 26)"));
        assert!(rendered.contains("Rgb(12, 42, 27)"));
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

    #[test]
    fn short_run_id_keeps_unique_suffix_visible() {
        assert_eq!(short_run_id("run-1777715825094-49196"), "run-...49196");
        assert_eq!(
            short_run_id_for_width("run-1777715825094-49196", 12),
            "run-...49196"
        );
        assert_eq!(short_run_id("manual"), "manual");
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

    fn sample_artifacts(metadata: RunMetadata) -> RunArtifacts {
        let run_dir = PathBuf::from(".keel/runs/run-1");
        RunArtifacts {
            report: ReportInfo::new(
                metadata,
                run_dir.join(artifact_files::REPORT),
                String::new(),
                artifacts_for_run_1(),
                Vec::new(),
            ),
            report_content: Some(String::new()),
            diff: Some(DiffInfo {
                path: run_dir.join(artifact_files::DIFF),
                content: "diff --git a/file b/file".to_string(),
                is_empty: false,
            }),
            log: None,
            checks: None,
        }
    }

    fn artifacts_for_run_1() -> Vec<ArtifactInfo> {
        RUN_ARTIFACTS.iter().map(artifact_for_spec).collect()
    }

    fn artifact_for_spec(spec: &RunArtifactSpec) -> ArtifactInfo {
        let run_dir = PathBuf::from(".keel/runs/run-1");
        ArtifactInfo::from_spec(spec, run_dir.join(spec.file), spec.required)
    }

    fn sample_run() -> RunMetadata {
        RunMetadata::new(
            "run-1",
            "task",
            "noop",
            RunStatus::Ready,
            "2026-05-01T00:00:00Z",
        )
        .with_base_commit("base")
    }

    fn sample_commit_artifact(metadata: &RunMetadata, commit_sha: &str) -> CommitArtifact {
        CommitArtifact {
            run_id: metadata.run_id.clone(),
            branch: metadata.branch.clone(),
            worktree: metadata.worktree_path.clone(),
            commit_sha: commit_sha.to_string(),
            commit_message: "keel: task".to_string(),
            committed_at: "2026-05-01T00:01:00Z".to_string(),
            had_uncommitted_changes: true,
            warnings: Vec::new(),
            dry_run: false,
        }
    }

    fn sample_push_artifact(
        metadata: &RunMetadata,
        commit_sha: &str,
        remote_url: &str,
    ) -> PushArtifact {
        PushArtifact {
            run_id: metadata.run_id.clone(),
            remote: "origin".to_string(),
            remote_url: remote_url.to_string(),
            branch: metadata.branch.clone(),
            commit_sha: commit_sha.to_string(),
            pushed: true,
            pushed_at: "2026-05-01T00:02:00Z".to_string(),
            dry_run: false,
        }
    }

    fn sample_pr_artifact(
        metadata: &RunMetadata,
        commit_sha: &str,
        remote_url: &str,
        pr_url: &str,
    ) -> PrArtifact {
        PrArtifact {
            run_id: metadata.run_id.clone(),
            provider: PrProvider::Github,
            provider_name: "GitHub".to_string(),
            request_kind: "pull_request".to_string(),
            remote: "origin".to_string(),
            remote_url: remote_url.to_string(),
            repository_url: Some("https://github.com/example/repo".to_string()),
            source_branch: metadata.branch.clone(),
            target_branch: "main".to_string(),
            commit_sha: commit_sha.to_string(),
            title: "keel: task".to_string(),
            url: pr_url.to_string(),
            created_at: "2026-05-01T00:03:00Z".to_string(),
            draft: true,
            reused_existing: false,
            dry_run: false,
        }
    }
}
