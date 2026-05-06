use anyhow::Result;
use keel_core::{KeelProject, RunArtifacts, RunMetadata, RunStatus};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TuiFilters {
    pub text: String,
    pub agent: Option<String>,
    pub status: Option<RunStatus>,
    pub run_id: Option<String>,
}

#[derive(Debug)]
pub struct App {
    project: KeelProject,
    runs: Vec<RunMetadata>,
    visible: Vec<usize>,
    selected: usize,
    run_list_offset: usize,
    tab: DetailTab,
    detail: Option<RunArtifacts>,
    message: Option<String>,
    filters: TuiFilters,
    filter_mode: bool,
    help_visible: bool,
    report_scroll: u16,
    diff_scroll: u16,
    log_scroll: u16,
    artifact_scroll: u16,
    report_scroll_max: u16,
    diff_scroll_max: u16,
    log_scroll_max: u16,
    artifact_scroll_max: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTab {
    Report,
    Diff,
    Log,
    Artifacts,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunCounts {
    pub ready: usize,
    pub not_ready: usize,
    pub running: usize,
    pub discarded: usize,
    pub committed: usize,
    pub pushed: usize,
    pub pr: usize,
}

impl App {
    pub fn load(project: KeelProject) -> Result<Self> {
        Self::load_with_filters(project, TuiFilters::default())
    }

    pub fn load_with_filter(project: KeelProject, filter: Option<String>) -> Result<Self> {
        Self::load_with_filters(
            project,
            TuiFilters {
                text: filter.unwrap_or_default(),
                ..TuiFilters::default()
            },
        )
    }

    pub fn load_with_filters(project: KeelProject, filters: TuiFilters) -> Result<Self> {
        let mut app = Self {
            project,
            runs: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            run_list_offset: 0,
            tab: DetailTab::Report,
            detail: None,
            message: None,
            filters,
            filter_mode: false,
            help_visible: false,
            report_scroll: 0,
            diff_scroll: 0,
            log_scroll: 0,
            artifact_scroll: 0,
            report_scroll_max: u16::MAX,
            diff_scroll_max: u16::MAX,
            log_scroll_max: u16::MAX,
            artifact_scroll_max: u16::MAX,
        };
        app.refresh()?;
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let selected_run_id = self.selected_run().map(|run| run.run_id.clone());
        self.runs = self.project.list_runs()?;
        self.rebuild_visible(selected_run_id);
        self.reload_detail();
        self.message = Some("refreshed".to_string());
        Ok(())
    }

    pub fn select_next(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.visible.len() - 1);
        self.clamp_run_list_offset();
        self.reload_detail();
    }

    pub fn select_previous(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
        self.clamp_run_list_offset();
        self.reload_detail();
    }

    pub fn select_first(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = 0;
        self.clamp_run_list_offset();
        self.reload_detail();
    }

    pub fn select_last(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = self.visible.len() - 1;
        self.clamp_run_list_offset();
        self.reload_detail();
    }

    pub fn next_tab(&mut self) {
        self.tab = match self.tab {
            DetailTab::Report => DetailTab::Diff,
            DetailTab::Diff => DetailTab::Log,
            DetailTab::Log => DetailTab::Artifacts,
            DetailTab::Artifacts => DetailTab::Report,
        };
    }

    pub fn previous_tab(&mut self) {
        self.tab = match self.tab {
            DetailTab::Report => DetailTab::Artifacts,
            DetailTab::Diff => DetailTab::Report,
            DetailTab::Log => DetailTab::Diff,
            DetailTab::Artifacts => DetailTab::Log,
        };
    }

    pub fn select_tab(&mut self, tab: DetailTab) {
        self.tab = tab;
    }

    pub fn runs(&self) -> &[RunMetadata] {
        &self.runs
    }

    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    pub fn total_count(&self) -> usize {
        self.runs.len()
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn run_list_offset(&self) -> usize {
        self.run_list_offset
    }

    pub fn selected_position(&self) -> Option<(usize, usize)> {
        (!self.visible.is_empty()).then_some((self.selected + 1, self.visible.len()))
    }

    pub fn selected_run(&self) -> Option<&RunMetadata> {
        self.visible
            .get(self.selected)
            .and_then(|index| self.runs.get(*index))
    }

    pub fn tab(&self) -> DetailTab {
        self.tab
    }

    pub fn detail(&self) -> Option<&RunArtifacts> {
        self.detail.as_ref()
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn filter(&self) -> &str {
        &self.filters.text
    }

    pub fn active_filter_label(&self) -> Option<String> {
        self.filters.label()
    }

    pub fn has_active_filters(&self) -> bool {
        self.filters.has_active()
    }

    pub fn filter_mode(&self) -> bool {
        self.filter_mode
    }

    pub fn apply_filter(&mut self, filter: impl Into<String>) {
        self.filters.text = filter.into();
        self.filter_mode = false;
        self.rebuild_visible(self.selected_run().map(|run| run.run_id.clone()));
        self.reload_detail();
        self.message = Some(self.filter_message());
    }

    pub fn help_visible(&self) -> bool {
        self.help_visible
    }

    pub fn toggle_help(&mut self) {
        self.help_visible = !self.help_visible;
    }

    pub fn close_help(&mut self) {
        self.help_visible = false;
    }

    pub fn begin_filter_edit(&mut self) {
        self.close_help();
        self.filter_mode = true;
        self.message =
            Some("filter mode: type to narrow runs, Enter to apply, Esc to clear".to_string());
    }

    pub fn finish_filter_edit(&mut self) {
        self.filter_mode = false;
        self.rebuild_visible(self.selected_run().map(|run| run.run_id.clone()));
        self.reload_detail();
        self.message = Some(self.filter_message());
    }

    pub fn clear_filter(&mut self) {
        self.filters.text.clear();
        self.filter_mode = false;
        self.rebuild_visible(self.selected_run().map(|run| run.run_id.clone()));
        self.reload_detail();
        self.message = Some(self.filter_message());
    }

    pub fn push_filter_char(&mut self, ch: char) {
        self.filters.text.push(ch);
        self.rebuild_visible(self.selected_run().map(|run| run.run_id.clone()));
        self.reload_detail();
        self.message = Some(self.filter_message());
    }

    pub fn pop_filter_char(&mut self) {
        self.filters.text.pop();
        self.rebuild_visible(self.selected_run().map(|run| run.run_id.clone()));
        self.reload_detail();
        self.message = Some(self.filter_message());
    }

    pub fn scroll_up(&mut self, amount: u16) {
        match self.tab {
            DetailTab::Report => self.report_scroll = self.report_scroll.saturating_sub(amount),
            DetailTab::Diff => self.diff_scroll = self.diff_scroll.saturating_sub(amount),
            DetailTab::Log => self.log_scroll = self.log_scroll.saturating_sub(amount),
            DetailTab::Artifacts => {
                self.artifact_scroll = self.artifact_scroll.saturating_sub(amount);
            }
        }
    }

    pub fn scroll_down(&mut self, amount: u16) {
        let max_scroll = self.scroll_max();
        match self.tab {
            DetailTab::Report => {
                self.report_scroll = self.report_scroll.saturating_add(amount).min(max_scroll);
            }
            DetailTab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_add(amount).min(max_scroll);
            }
            DetailTab::Log => {
                self.log_scroll = self.log_scroll.saturating_add(amount).min(max_scroll);
            }
            DetailTab::Artifacts => {
                self.artifact_scroll = self.artifact_scroll.saturating_add(amount).min(max_scroll);
            }
        }
    }

    pub fn scroll_home(&mut self) {
        match self.tab {
            DetailTab::Report => self.report_scroll = 0,
            DetailTab::Diff => self.diff_scroll = 0,
            DetailTab::Log => self.log_scroll = 0,
            DetailTab::Artifacts => self.artifact_scroll = 0,
        }
    }

    pub fn scroll_end(&mut self) {
        let max_scroll = self.scroll_max();
        match self.tab {
            DetailTab::Report => self.report_scroll = max_scroll,
            DetailTab::Diff => self.diff_scroll = max_scroll,
            DetailTab::Log => self.log_scroll = max_scroll,
            DetailTab::Artifacts => self.artifact_scroll = max_scroll,
        }
    }

    pub fn scroll_offset(&self) -> u16 {
        match self.tab {
            DetailTab::Report => self.report_scroll,
            DetailTab::Diff => self.diff_scroll,
            DetailTab::Log => self.log_scroll,
            DetailTab::Artifacts => self.artifact_scroll,
        }
    }

    pub fn set_scroll_limit(&mut self, content_lines: usize, visible_rows: u16) {
        let visible_rows = usize::from(visible_rows.max(1));
        let max_scroll = content_lines
            .saturating_sub(visible_rows)
            .min(usize::from(u16::MAX)) as u16;

        match self.tab {
            DetailTab::Report => {
                self.report_scroll_max = max_scroll;
                self.report_scroll = self.report_scroll.min(max_scroll);
            }
            DetailTab::Diff => {
                self.diff_scroll_max = max_scroll;
                self.diff_scroll = self.diff_scroll.min(max_scroll);
            }
            DetailTab::Log => {
                self.log_scroll_max = max_scroll;
                self.log_scroll = self.log_scroll.min(max_scroll);
            }
            DetailTab::Artifacts => {
                self.artifact_scroll_max = max_scroll;
                self.artifact_scroll = self.artifact_scroll.min(max_scroll);
            }
        }
    }

    pub fn set_run_list_viewport(&mut self, visible_rows: usize) {
        let visible_rows = visible_rows.max(1);
        if self.visible.is_empty() {
            self.run_list_offset = 0;
            return;
        }

        if self.selected < self.run_list_offset {
            self.run_list_offset = self.selected;
        } else if self.selected >= self.run_list_offset + visible_rows {
            self.run_list_offset = self.selected + 1 - visible_rows;
        }

        let max_offset = self.visible.len().saturating_sub(visible_rows);
        self.run_list_offset = self.run_list_offset.min(max_offset);
    }

    pub fn visible_run_window(&self, visible_rows: usize) -> Vec<(usize, &RunMetadata)> {
        let visible_rows = visible_rows.max(1);
        self.visible
            .iter()
            .enumerate()
            .skip(self.run_list_offset)
            .take(visible_rows)
            .filter_map(|(visible_index, index)| {
                self.runs.get(*index).map(|run| (visible_index, run))
            })
            .collect()
    }

    pub fn counts(&self) -> RunCounts {
        self.runs
            .iter()
            .fold(RunCounts::default(), |mut counts, run| {
                match run.status {
                    RunStatus::Ready => counts.ready += 1,
                    RunStatus::NotReady => counts.not_ready += 1,
                    RunStatus::Running => counts.running += 1,
                    RunStatus::Discarded => counts.discarded += 1,
                    RunStatus::Created => {}
                }
                if run.has_commit_record() {
                    counts.committed += 1;
                }
                if run.has_push_record() {
                    counts.pushed += 1;
                }
                if run.has_pr_record() {
                    counts.pr += 1;
                }
                counts
            })
    }

    fn reload_detail(&mut self) {
        let Some(run_id) = self.selected_run().map(|run| run.run_id.clone()) else {
            self.detail = None;
            return;
        };

        match self.project.run_artifacts(&run_id) {
            Ok(detail) => {
                self.detail = Some(detail);
                self.message = None;
            }
            Err(error) => {
                self.detail = None;
                self.message = Some(error.to_string());
            }
        }
    }

    fn clamp_selection(&mut self) {
        if self.visible.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.visible.len() {
            self.selected = self.visible.len() - 1;
        }
        self.clamp_run_list_offset();
    }

    fn clamp_run_list_offset(&mut self) {
        if self.visible.is_empty() {
            self.run_list_offset = 0;
        } else if self.run_list_offset > self.selected {
            self.run_list_offset = self.selected;
        }
    }

    fn rebuild_visible(&mut self, selected_run_id: Option<String>) {
        self.visible = self
            .runs
            .iter()
            .enumerate()
            .filter_map(|(index, run)| self.run_matches_filter(run).then_some(index))
            .collect();

        self.selected = selected_run_id
            .and_then(|run_id| {
                self.visible
                    .iter()
                    .position(|index| self.runs[*index].run_id == run_id)
            })
            .or_else(|| self.preferred_review_selection())
            .unwrap_or(0);
        self.clamp_selection();
    }

    fn preferred_review_selection(&self) -> Option<usize> {
        self.visible
            .iter()
            .position(|index| self.runs[*index].status == RunStatus::Ready)
    }

    fn run_matches_filter(&self, run: &RunMetadata) -> bool {
        if let Some(run_id) = self.filters.run_id.as_deref() {
            if run.run_id != run_id {
                return false;
            }
        }

        if let Some(agent) = self.filters.agent.as_deref() {
            if run.agent != agent {
                return false;
            }
        }

        if let Some(status) = self.filters.status.as_ref() {
            if &run.status != status {
                return false;
            }
        }

        let filter = self.filters.text.trim().to_ascii_lowercase();
        if filter.is_empty() {
            return true;
        }

        run.review_search_terms()
            .iter()
            .any(|haystack| haystack.to_ascii_lowercase().contains(&filter))
            || run
                .warnings
                .iter()
                .any(|warning| warning.to_ascii_lowercase().contains(&filter))
            || run.risk_warnings.iter().any(|warning| {
                warning.kind.to_string().contains(&filter)
                    || warning.message.to_ascii_lowercase().contains(&filter)
                    || warning
                        .path
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(&filter)
                    || warning
                        .pattern
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(&filter)
                    || warning
                        .details
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(&filter)
            })
    }

    fn filter_message(&self) -> String {
        let Some(label) = self.filters.label() else {
            return "filter cleared".to_string();
        };

        format!(
            "filter: {label} ({} of {} runs)",
            self.visible.len(),
            self.runs.len()
        )
    }

    fn scroll_max(&self) -> u16 {
        match self.tab {
            DetailTab::Report => self.report_scroll_max,
            DetailTab::Diff => self.diff_scroll_max,
            DetailTab::Log => self.log_scroll_max,
            DetailTab::Artifacts => self.artifact_scroll_max,
        }
    }
}

impl TuiFilters {
    fn has_active(&self) -> bool {
        !self.text.trim().is_empty()
            || self.agent.is_some()
            || self.status.is_some()
            || self.run_id.is_some()
    }

    fn label(&self) -> Option<String> {
        let mut parts = Vec::new();
        let text = self.text.trim();
        if !text.is_empty() {
            parts.push(text.to_string());
        }
        if let Some(agent) = self.agent.as_deref() {
            parts.push(format!("agent: {agent}"));
        }
        if let Some(status) = self.status.as_ref() {
            parts.push(format!("status: {status}"));
        }
        if let Some(run_id) = self.run_id.as_deref() {
            parts.push(format!("run: {run_id}"));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

impl DetailTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Report => "Report",
            Self::Diff => "Diff",
            Self::Log => "Log",
            Self::Artifacts => "Artifacts",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keel_core::{
        artifact_files, ArtifactInfo, CheckResult, CheckStatus, DiffInfo, LogInfo, ReportInfo,
        RunArtifactSpec, RunMetadata, RUN_ARTIFACTS,
    };
    use std::path::PathBuf;

    #[test]
    fn counts_include_review_and_git_states() {
        let mut app = empty_app();
        app.runs = vec![
            sample_run("a", RunStatus::Ready, true, true, true),
            sample_run("b", RunStatus::NotReady, false, false, false),
            sample_run("c", RunStatus::Discarded, true, false, false),
        ];

        let counts = app.counts();

        assert_eq!(counts.ready, 1);
        assert_eq!(counts.not_ready, 1);
        assert_eq!(counts.discarded, 1);
        assert_eq!(counts.committed, 2);
        assert_eq!(counts.pushed, 1);
        assert_eq!(counts.pr, 1);
    }

    #[test]
    fn tab_navigation_wraps() {
        let mut app = empty_app();

        app.previous_tab();
        assert_eq!(app.tab(), DetailTab::Artifacts);

        app.next_tab();
        assert_eq!(app.tab(), DetailTab::Report);
    }

    #[test]
    fn direct_tab_selection_opens_requested_tab() {
        let mut app = empty_app();

        app.select_tab(DetailTab::Log);
        assert_eq!(app.tab(), DetailTab::Log);

        app.select_tab(DetailTab::Diff);
        assert_eq!(app.tab(), DetailTab::Diff);

        app.select_tab(DetailTab::Artifacts);
        assert_eq!(app.tab(), DetailTab::Artifacts);
    }

    fn empty_app() -> App {
        empty_app_with_filters(TuiFilters::default())
    }

    fn empty_app_with_filters(filters: TuiFilters) -> App {
        App {
            project: KeelProject::from_root_for_display("."),
            runs: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            run_list_offset: 0,
            tab: DetailTab::Report,
            detail: None,
            message: None,
            filters,
            filter_mode: false,
            help_visible: false,
            report_scroll: 0,
            diff_scroll: 0,
            log_scroll: 0,
            artifact_scroll: 0,
            report_scroll_max: u16::MAX,
            diff_scroll_max: u16::MAX,
            log_scroll_max: u16::MAX,
            artifact_scroll_max: u16::MAX,
        }
    }

    #[test]
    fn filter_narrows_visible_runs_and_clear_restores_all() {
        let mut app = empty_app();
        app.runs = vec![
            sample_run("a", RunStatus::Ready, false, false, false),
            sample_run("b", RunStatus::NotReady, false, false, false),
        ];
        app.runs[0].task = "fix config parser".to_string();
        app.runs[1].task = "update readme".to_string();
        app.rebuild_visible(None);

        app.begin_filter_edit();
        for ch in "config".chars() {
            app.push_filter_char(ch);
        }

        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_run().unwrap().run_id, "a");

        app.clear_filter();

        assert_eq!(app.visible_count(), 2);
        assert_eq!(app.filter(), "");
    }

    #[test]
    fn help_overlay_toggles_and_filter_mode_closes_it() {
        let mut app = empty_app();

        assert!(!app.help_visible());

        app.toggle_help();
        assert!(app.help_visible());

        app.toggle_help();
        assert!(!app.help_visible());

        app.toggle_help();
        app.begin_filter_edit();

        assert!(app.filter_mode());
        assert!(!app.help_visible());
    }

    #[test]
    fn initial_filter_uses_existing_filter_matching() {
        let mut app = empty_app();
        app.runs = vec![
            sample_run("run-auth", RunStatus::Ready, false, false, false),
            sample_run("run-docs", RunStatus::Ready, false, false, false),
        ];
        app.runs[0].task = "fix auth flow".to_string();
        app.runs[1].task = "update docs".to_string();

        app.apply_filter("auth");

        assert_eq!(app.filter(), "auth");
        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_run().unwrap().run_id, "run-auth");
    }

    #[test]
    fn startup_filters_match_agent_and_status_exactly() {
        let mut app = empty_app_with_filters(TuiFilters {
            agent: Some("codex".to_string()),
            status: Some(RunStatus::Ready),
            ..TuiFilters::default()
        });
        app.runs = vec![
            sample_run_with_agent("run-noop-ready", "noop", RunStatus::Ready),
            sample_run_with_agent("run-codex-ready", "codex", RunStatus::Ready),
            sample_run_with_agent("run-codex-failed", "codex", RunStatus::NotReady),
        ];
        app.rebuild_visible(None);

        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_run().unwrap().run_id, "run-codex-ready");
    }

    #[test]
    fn startup_run_filter_focuses_exact_run_id() {
        let mut app = empty_app_with_filters(TuiFilters {
            run_id: Some("run-target".to_string()),
            ..TuiFilters::default()
        });
        app.runs = vec![
            sample_run("run-other", RunStatus::Ready, false, false, false),
            sample_run("run-target", RunStatus::Ready, false, false, false),
            sample_run("run-target-extra", RunStatus::Ready, false, false, false),
        ];
        app.rebuild_visible(None);

        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_run().unwrap().run_id, "run-target");
        assert!(app.has_active_filters());
        assert_eq!(app.active_filter_label().unwrap(), "run: run-target");
    }

    #[test]
    fn default_selection_prefers_newest_ready_run() {
        let mut app = empty_app();
        app.runs = vec![
            sample_run(
                "run-newest-blocked",
                RunStatus::NotReady,
                false,
                false,
                false,
            ),
            sample_run("run-newest-ready", RunStatus::Ready, false, false, false),
            sample_run("run-older-ready", RunStatus::Ready, false, false, false),
        ];
        app.rebuild_visible(None);

        assert_eq!(app.visible_count(), 3);
        assert_eq!(app.selected_run().unwrap().run_id, "run-newest-ready");
    }

    #[test]
    fn startup_filters_combine_with_text_filter() {
        let mut app = empty_app_with_filters(TuiFilters {
            text: "auth".to_string(),
            agent: Some("noop".to_string()),
            status: Some(RunStatus::Ready),
            ..TuiFilters::default()
        });
        app.runs = vec![
            sample_run_with_task("run-auth-ready", "noop", RunStatus::Ready, "fix auth"),
            sample_run_with_task("run-docs-ready", "noop", RunStatus::Ready, "update docs"),
            sample_run_with_task("run-auth-codex", "codex", RunStatus::Ready, "fix auth"),
            sample_run_with_task("run-auth-failed", "noop", RunStatus::NotReady, "fix auth"),
        ];
        app.rebuild_visible(None);

        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_run().unwrap().run_id, "run-auth-ready");
        assert_eq!(
            app.active_filter_label().unwrap(),
            "auth, agent: noop, status: ready"
        );
    }

    #[test]
    fn clearing_text_filter_preserves_structured_startup_filters() {
        let mut app = empty_app_with_filters(TuiFilters {
            text: "auth".to_string(),
            agent: Some("noop".to_string()),
            status: Some(RunStatus::Ready),
            ..TuiFilters::default()
        });
        app.runs = vec![
            sample_run_with_task("run-auth-ready", "noop", RunStatus::Ready, "fix auth"),
            sample_run_with_task("run-docs-ready", "noop", RunStatus::Ready, "update docs"),
            sample_run_with_task("run-auth-codex", "codex", RunStatus::Ready, "fix auth"),
        ];
        app.rebuild_visible(None);

        app.clear_filter();

        assert_eq!(app.filter(), "");
        assert_eq!(app.visible_count(), 2);
        assert_eq!(
            app.active_filter_label().unwrap(),
            "agent: noop, status: ready"
        );
    }

    #[test]
    fn text_filter_uses_nested_review_artifact_terms() {
        let mut app = empty_app();
        let mut run = sample_run("run-review-state", RunStatus::Ready, false, false, false);
        run.commit = Some(keel_core::CommitArtifact {
            run_id: run.run_id.clone(),
            branch: run.branch.clone(),
            worktree: run.worktree_path.clone(),
            commit_sha: "abc123".to_string(),
            commit_message: "keel: task".to_string(),
            committed_at: "2026-05-01T00:01:00Z".to_string(),
            had_uncommitted_changes: true,
            warnings: Vec::new(),
            dry_run: false,
        });
        run.push = Some(keel_core::PushArtifact {
            run_id: run.run_id.clone(),
            remote: "origin".to_string(),
            remote_url: "git@github.com:owner/repo.git".to_string(),
            branch: run.branch.clone(),
            commit_sha: "abc123".to_string(),
            pushed: true,
            pushed_at: "2026-05-01T00:02:00Z".to_string(),
            dry_run: false,
        });
        run.pr = Some(keel_core::PrArtifact {
            run_id: run.run_id.clone(),
            provider: keel_core::PrProvider::Github,
            provider_name: "GitHub".to_string(),
            request_kind: "pull_request".to_string(),
            remote: "origin".to_string(),
            remote_url: "git@github.com:owner/repo.git".to_string(),
            repository_url: Some("https://github.com/owner/repo".to_string()),
            source_branch: run.branch.clone(),
            target_branch: "main".to_string(),
            commit_sha: "abc123".to_string(),
            title: "keel: task".to_string(),
            url: "https://github.com/owner/repo/pull/1".to_string(),
            created_at: "2026-05-01T00:03:00Z".to_string(),
            draft: true,
            reused_existing: false,
            dry_run: false,
        });
        app.runs = vec![run];

        app.apply_filter("pull/1");

        assert_eq!(app.visible_count(), 1);
        assert_eq!(app.selected_run().unwrap().run_id, "run-review-state");
    }

    #[test]
    fn scroll_state_is_per_text_tab() {
        let mut app = empty_app();

        app.scroll_down(6);
        assert_eq!(app.scroll_offset(), 6);

        app.next_tab();
        assert_eq!(app.tab(), DetailTab::Diff);
        app.scroll_down(10);
        assert_eq!(app.scroll_offset(), 10);

        app.next_tab();
        assert_eq!(app.tab(), DetailTab::Log);
        assert_eq!(app.scroll_offset(), 0);
        app.scroll_down(4);
        assert_eq!(app.scroll_offset(), 4);

        app.previous_tab();
        assert_eq!(app.tab(), DetailTab::Diff);
        assert_eq!(app.scroll_offset(), 10);

        app.previous_tab();
        assert_eq!(app.tab(), DetailTab::Report);
        assert_eq!(app.scroll_offset(), 6);

        app.previous_tab();
        assert_eq!(app.tab(), DetailTab::Artifacts);
        assert_eq!(app.scroll_offset(), 0);
        app.scroll_end();
        assert_eq!(app.scroll_offset(), u16::MAX);
    }

    #[test]
    fn rendered_scroll_bounds_keep_page_up_responsive() {
        let mut app = empty_app();
        let mut run = sample_run("run-scrolled", RunStatus::NotReady, false, false, false);
        run.failure_reason = Some(keel_core::FailureReason::CheckFailed);
        run.readiness_reason = "failed checks: cargo test".to_string();
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_not_ready_artifacts(run));

        crate::ui::render_to_string(&mut app, 92, 28);
        app.scroll_down(999);
        let bottom = app.scroll_offset();

        assert!(bottom < 999);

        app.scroll_up(15);

        assert!(app.scroll_offset() < bottom);
    }

    #[test]
    fn report_and_artifact_tabs_keep_independent_scroll_state() {
        let mut app = empty_app();

        app.scroll_down(3);
        assert_eq!(app.scroll_offset(), 3);

        app.previous_tab();
        assert_eq!(app.tab(), DetailTab::Artifacts);
        assert_eq!(app.scroll_offset(), 0);

        app.scroll_down(7);
        assert_eq!(app.scroll_offset(), 7);

        app.next_tab();
        assert_eq!(app.tab(), DetailTab::Report);
        assert_eq!(app.scroll_offset(), 3);
    }

    #[test]
    fn run_list_window_keeps_selected_run_visible() {
        let mut app = empty_app();
        app.runs = (0..12)
            .map(|index| {
                sample_run(
                    &format!("run-{index}"),
                    RunStatus::Ready,
                    false,
                    false,
                    false,
                )
            })
            .collect();
        app.rebuild_visible(None);
        app.set_run_list_viewport(4);

        assert_eq!(app.run_list_offset(), 0);

        for _ in 0..7 {
            app.select_next();
        }
        app.set_run_list_viewport(4);

        assert_eq!(app.selected_index(), 7);
        assert_eq!(app.run_list_offset(), 4);

        let window = app.visible_run_window(4);
        assert_eq!(window.first().unwrap().1.run_id, "run-4");
        assert_eq!(window.last().unwrap().1.run_id, "run-7");

        for _ in 0..6 {
            app.select_previous();
        }
        app.set_run_list_viewport(4);

        assert_eq!(app.selected_index(), 1);
        assert_eq!(app.run_list_offset(), 1);
    }

    #[test]
    fn first_and_last_run_selection_jump_visible_runs() {
        let mut app = empty_app();
        app.runs = (0..5)
            .map(|index| {
                sample_run(
                    &format!("run-{index}"),
                    RunStatus::Ready,
                    false,
                    false,
                    false,
                )
            })
            .collect();
        app.rebuild_visible(None);

        app.select_last();
        assert_eq!(app.selected_run().unwrap().run_id, "run-4");

        app.select_first();
        assert_eq!(app.selected_run().unwrap().run_id, "run-0");
    }

    #[test]
    fn first_and_last_selection_ignore_empty_run_lists() {
        let mut app = empty_app();

        app.select_last();
        assert_eq!(app.selected_index(), 0);

        app.select_first();
        assert_eq!(app.selected_index(), 0);
    }

    #[test]
    fn render_snapshot_covers_read_only_review_layout() {
        let mut app = empty_app();
        let run = sample_run("run-123", RunStatus::Ready, true, true, false);
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_artifacts(run));

        insta::assert_snapshot!(
            "tui_read_only_review_layout",
            crate::ui::render_to_string(&mut app, 120, 32)
        );
    }

    #[test]
    fn render_snapshot_covers_not_ready_failure_summary() {
        let mut app = empty_app();
        let mut run = sample_run("run-failed", RunStatus::NotReady, false, false, false);
        run.failure_reason = Some(keel_core::FailureReason::CheckFailed);
        run.readiness_reason = "failed checks: cargo test".to_string();
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_not_ready_artifacts(run));

        insta::assert_snapshot!(
            "tui_not_ready_failure_summary",
            crate::ui::render_to_string(&mut app, 120, 32)
        );
    }

    #[test]
    fn render_snapshot_covers_narrow_review_layout() {
        let mut app = empty_app();
        let mut run = sample_run("run-narrow", RunStatus::NotReady, false, false, false);
        run.task = "investigate failing narrow terminal review layout".to_string();
        run.failure_reason = Some(keel_core::FailureReason::CheckFailed);
        run.readiness_reason = "failed checks: cargo test".to_string();
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_not_ready_artifacts(run));

        insta::assert_snapshot!(
            "tui_narrow_review_layout",
            crate::ui::render_to_string(&mut app, 92, 34)
        );
    }

    #[test]
    fn render_snapshot_covers_scrolled_report_summary() {
        let mut app = empty_app();
        let mut run = sample_run("run-scrolled", RunStatus::NotReady, false, false, false);
        run.failure_reason = Some(keel_core::FailureReason::CheckFailed);
        run.readiness_reason = "failed checks: cargo test".to_string();
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_not_ready_artifacts(run));
        app.scroll_down(4);

        insta::assert_snapshot!(
            "tui_scrolled_report_summary",
            crate::ui::render_to_string(&mut app, 92, 28)
        );
    }

    #[test]
    fn render_snapshot_covers_help_overlay() {
        let mut app = empty_app();
        let run = sample_run("run-help", RunStatus::Ready, false, false, false);
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_artifacts(run));
        app.toggle_help();

        insta::assert_snapshot!(
            "tui_help_overlay",
            crate::ui::render_to_string(&mut app, 120, 34)
        );
    }

    #[test]
    fn render_snapshot_covers_artifact_groups() {
        let mut app = empty_app();
        let run = sample_run("run-123", RunStatus::Ready, true, false, false);
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_artifacts(run));
        app.previous_tab();

        insta::assert_snapshot!(
            "tui_artifact_groups",
            crate::ui::render_to_string(&mut app, 120, 32)
        );
    }

    #[test]
    fn render_snapshot_covers_colored_diff_review() {
        let mut app = empty_app();
        let run = sample_run("run-diff", RunStatus::Ready, false, false, false);
        app.runs = vec![run.clone()];
        app.rebuild_visible(None);
        app.detail = Some(sample_artifacts(run));
        app.next_tab();

        insta::assert_snapshot!(
            "tui_colored_diff_review",
            crate::ui::render_to_string(&mut app, 120, 32)
        );
    }

    #[test]
    fn render_snapshot_covers_filtered_empty_state() {
        let mut app = empty_app_with_filters(TuiFilters {
            agent: Some("codex".to_string()),
            status: Some(RunStatus::Ready),
            ..TuiFilters::default()
        });
        app.runs = vec![sample_run_with_agent(
            "run-filtered",
            "noop",
            RunStatus::NotReady,
        )];
        app.rebuild_visible(None);

        insta::assert_snapshot!(
            "tui_filtered_empty_state",
            crate::ui::render_to_string(&mut app, 120, 26)
        );
    }

    fn sample_run(
        run_id: &str,
        status: RunStatus,
        committed: bool,
        pushed: bool,
        pr_created: bool,
    ) -> RunMetadata {
        sample_run_with_agent_and_task(
            run_id, "noop", status, "task", committed, pushed, pr_created,
        )
    }

    fn sample_run_with_agent(run_id: &str, agent: &str, status: RunStatus) -> RunMetadata {
        sample_run_with_agent_and_task(run_id, agent, status, "task", false, false, false)
    }

    fn sample_run_with_task(
        run_id: &str,
        agent: &str,
        status: RunStatus,
        task: &str,
    ) -> RunMetadata {
        sample_run_with_agent_and_task(run_id, agent, status, task, false, false, false)
    }

    fn sample_run_with_agent_and_task(
        run_id: &str,
        agent: &str,
        status: RunStatus,
        task: &str,
        committed: bool,
        pushed: bool,
        pr_created: bool,
    ) -> RunMetadata {
        let mut metadata = RunMetadata::new(run_id, task, agent, status, "2026-05-01T00:00:00Z")
            .with_base_commit("base");
        metadata.committed = committed;
        metadata.pushed = pushed;
        metadata.pr_created = pr_created;
        metadata
    }

    fn sample_artifacts(metadata: RunMetadata) -> RunArtifacts {
        let run_dir = PathBuf::from(format!(".keel/runs/{}", metadata.run_id));
        RunArtifacts {
            report: sample_report_info(
                metadata.clone(),
                run_dir.join(artifact_files::REPORT),
                "sample summary",
                artifacts_for(&metadata),
                vec!["keel diff run-123".to_string()],
            ),
            report_content: Some("# Keel Run Report\n".to_string()),
            diff: Some(DiffInfo {
                path: run_dir.join(artifact_files::DIFF),
                content: [
                    "diff --git a/file b/file",
                    "index 1111111..2222222 100644",
                    "--- a/file",
                    "+++ b/file",
                    "@@ -1,2 +1,2 @@",
                    "-old line",
                    "+new line",
                    " context",
                ]
                .join("\n"),
                is_empty: false,
            }),
            log: Some(LogInfo {
                path: run_dir.join(artifact_files::LOG),
                content: "created run run-123\n".to_string(),
                is_empty: false,
            }),
            checks: Some(vec![CheckResult {
                name: "cargo test".to_string(),
                command: "cargo test".to_string(),
                status: CheckStatus::Passed,
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            }]),
        }
    }

    fn sample_not_ready_artifacts(metadata: RunMetadata) -> RunArtifacts {
        let run_dir = PathBuf::from(format!(".keel/runs/{}", metadata.run_id));
        RunArtifacts {
            report: sample_report_info(
                metadata.clone(),
                run_dir.join(artifact_files::REPORT),
                "failed checks: cargo test",
                artifacts_for(&metadata),
                vec!["keel log run-failed".to_string()],
            ),
            report_content: Some("# Keel Run Report\n".to_string()),
            diff: Some(DiffInfo {
                path: run_dir.join(artifact_files::DIFF),
                content: [
                    "diff --git a/file b/file",
                    "index 1111111..2222222 100644",
                    "--- a/file",
                    "+++ b/file",
                    "@@ -1,2 +1,2 @@",
                    "-old line",
                    "+new line",
                    " context",
                ]
                .join("\n"),
                is_empty: false,
            }),
            log: Some(LogInfo {
                path: run_dir.join(artifact_files::LOG),
                content: "cargo test failed\n".to_string(),
                is_empty: false,
            }),
            checks: Some(vec![
                CheckResult {
                    name: "git status".to_string(),
                    command: "git status --short".to_string(),
                    status: CheckStatus::Passed,
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                },
                CheckResult {
                    name: "cargo test".to_string(),
                    command: "cargo test".to_string(),
                    status: CheckStatus::Failed,
                    exit_code: Some(101),
                    stdout: String::new(),
                    stderr: String::new(),
                },
            ]),
        }
    }

    fn artifacts_for(metadata: &RunMetadata) -> Vec<ArtifactInfo> {
        RUN_ARTIFACTS
            .iter()
            .map(|spec| artifact_for_spec(metadata, spec))
            .collect()
    }

    fn artifact_for_spec(metadata: &RunMetadata, spec: &RunArtifactSpec) -> ArtifactInfo {
        ArtifactInfo::from_spec(
            spec,
            PathBuf::from(format!(".keel/runs/{}", metadata.run_id)).join(spec.file),
            artifact_exists_for_metadata(metadata, spec),
        )
    }

    fn artifact_exists_for_metadata(metadata: &RunMetadata, spec: &RunArtifactSpec) -> bool {
        metadata.run_artifact_recorded(spec.key)
    }

    fn sample_report_info(
        metadata: RunMetadata,
        path: PathBuf,
        summary: &str,
        artifacts: Vec<ArtifactInfo>,
        next_actions: Vec<String>,
    ) -> ReportInfo {
        ReportInfo::new(metadata, path, summary, artifacts, next_actions)
    }
}
