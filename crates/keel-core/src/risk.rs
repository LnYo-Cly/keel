use crate::config::RiskConfig;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const DEPENDENCY_MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
];
const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "uv.lock",
];
const BUILT_IN_HIGH_RISK_PATHS: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "COPILOT.md",
    ".github/**",
    ".keel/**",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskWarning {
    pub kind: RiskWarningKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskWarningKind {
    RiskPath,
    DependencyManifest,
    Lockfile,
    DeletedFile,
    LargeDiff,
    HighRiskPath,
    InvalidRiskPattern,
}

impl std::fmt::Display for RiskWarningKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::RiskPath => "risk_path",
            Self::DependencyManifest => "dependency_manifest",
            Self::Lockfile => "lockfile",
            Self::DeletedFile => "deleted_file",
            Self::LargeDiff => "large_diff",
            Self::HighRiskPath => "high_risk_path",
            Self::InvalidRiskPattern => "invalid_risk_pattern",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangedFile {
    path: String,
    deleted: bool,
}

pub fn analyze_diff_risk(diff: &str, config: &RiskConfig) -> Vec<RiskWarning> {
    let changed_files = changed_files_from_diff(diff);
    if changed_files.is_empty() {
        return Vec::new();
    }

    let mut warnings = Vec::new();
    let (risk_globs, risk_patterns, invalid_patterns) = compile_globs(&config.paths);
    warnings.extend(invalid_patterns);

    let (built_in_globs, built_in_patterns, _) = compile_globs(BUILT_IN_HIGH_RISK_PATHS);

    for file in &changed_files {
        add_configured_risk_path_warnings(file, &risk_globs, &risk_patterns, &mut warnings);
        add_built_in_high_risk_path_warnings(
            file,
            &built_in_globs,
            &built_in_patterns,
            &mut warnings,
        );
        add_dependency_warning(file, &mut warnings);
        add_lockfile_warning(file, &mut warnings);
        add_deleted_file_warning(file, &mut warnings);
    }

    add_large_diff_warning(
        &changed_files,
        config.large_diff_file_threshold,
        &mut warnings,
    );
    warnings
}

pub fn format_risk_warning(warning: &RiskWarning) -> String {
    let mut output = warning.message.clone();
    if let Some(details) = &warning.details {
        output.push_str(": ");
        output.push_str(details);
    }
    output
}

fn add_configured_risk_path_warnings(
    file: &ChangedFile,
    globs: &GlobSet,
    patterns: &[String],
    warnings: &mut Vec<RiskWarning>,
) {
    for index in globs.matches(&file.path) {
        let pattern = patterns[index].clone();
        warnings.push(RiskWarning::new(
            RiskWarningKind::RiskPath,
            format!("touched risk path: {} matched {}", file.path, pattern),
            Some(file.path.clone()),
            Some(pattern),
            None,
        ));
    }
}

fn add_built_in_high_risk_path_warnings(
    file: &ChangedFile,
    globs: &GlobSet,
    patterns: &[String],
    warnings: &mut Vec<RiskWarning>,
) {
    for index in globs.matches(&file.path) {
        let pattern = patterns[index].clone();
        warnings.push(RiskWarning::new(
            RiskWarningKind::HighRiskPath,
            format!("high-risk path changed: {}", file.path),
            Some(file.path.clone()),
            Some(pattern),
            None,
        ));
    }
}

fn add_dependency_warning(file: &ChangedFile, warnings: &mut Vec<RiskWarning>) {
    if path_matches_name(&file.path, DEPENDENCY_MANIFESTS) {
        warnings.push(RiskWarning::new(
            RiskWarningKind::DependencyManifest,
            format!("dependency manifest changed: {}", file.path),
            Some(file.path.clone()),
            None,
            None,
        ));
    }
}

fn add_lockfile_warning(file: &ChangedFile, warnings: &mut Vec<RiskWarning>) {
    if path_matches_name(&file.path, LOCKFILES) {
        warnings.push(RiskWarning::new(
            RiskWarningKind::Lockfile,
            format!("lockfile changed: {}", file.path),
            Some(file.path.clone()),
            None,
            None,
        ));
    }
}

fn add_deleted_file_warning(file: &ChangedFile, warnings: &mut Vec<RiskWarning>) {
    if file.deleted {
        warnings.push(RiskWarning::new(
            RiskWarningKind::DeletedFile,
            format!("deleted file detected: {}", file.path),
            Some(file.path.clone()),
            None,
            None,
        ));
    }
}

fn add_large_diff_warning(
    changed_files: &[ChangedFile],
    threshold: usize,
    warnings: &mut Vec<RiskWarning>,
) {
    if threshold > 0 && changed_files.len() > threshold {
        warnings.push(RiskWarning::new(
            RiskWarningKind::LargeDiff,
            format!(
                "large diff: {} changed files exceeds threshold {}",
                changed_files.len(),
                threshold
            ),
            None,
            None,
            Some(format!("changed_file_count={}", changed_files.len())),
        ));
    }
}

fn changed_files_from_diff(diff: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut current: Option<ChangedFile> = None;

    for line in diff.lines() {
        if let Some(path) = parse_diff_git_path(line) {
            push_changed_file(&mut files, current.take());
            current = Some(ChangedFile {
                path,
                deleted: false,
            });
            continue;
        }

        if line.starts_with("deleted file mode") {
            if let Some(file) = &mut current {
                file.deleted = true;
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("rename to ") {
            if let Some(file) = &mut current {
                file.path = normalize_diff_path(path);
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ ") {
            if path.trim() == "/dev/null" {
                if let Some(file) = &mut current {
                    file.deleted = true;
                }
            } else if let Some(file) = &mut current {
                file.path = strip_diff_path_prefix(path);
            }
        }
    }

    push_changed_file(&mut files, current);
    deduplicate_changed_files(files)
}

fn parse_diff_git_path(line: &str) -> Option<String> {
    let rest = line.strip_prefix("diff --git ")?;
    if let Some((_, right)) = rest.rsplit_once(" b/") {
        return Some(normalize_diff_path(right));
    }

    let mut parts = rest.split_whitespace();
    let _left = parts.next()?;
    let right = parts.next()?;
    Some(strip_diff_path_prefix(right))
}

fn strip_diff_path_prefix(path: &str) -> String {
    let path = path.trim().trim_matches('"');
    normalize_diff_path(path.strip_prefix("b/").unwrap_or(path))
}

fn normalize_diff_path(path: &str) -> String {
    path.trim().trim_matches('"').replace('\\', "/")
}

fn push_changed_file(files: &mut Vec<ChangedFile>, file: Option<ChangedFile>) {
    let Some(file) = file else {
        return;
    };
    if !file.path.trim().is_empty() && file.path != "/dev/null" {
        files.push(file);
    }
}

fn deduplicate_changed_files(files: Vec<ChangedFile>) -> Vec<ChangedFile> {
    let mut positions: HashMap<String, usize> = HashMap::new();
    let mut deduped: Vec<ChangedFile> = Vec::new();
    for file in files {
        if let Some(index) = positions.get(&file.path).copied() {
            deduped[index].deleted |= file.deleted;
        } else {
            positions.insert(file.path.clone(), deduped.len());
            deduped.push(file);
        }
    }
    deduped
}

fn compile_globs(patterns: &[impl AsRef<str>]) -> (GlobSet, Vec<String>, Vec<RiskWarning>) {
    let mut builder = GlobSetBuilder::new();
    let mut valid_patterns = Vec::new();
    let mut warnings = Vec::new();

    for pattern in patterns {
        let pattern = pattern.as_ref();
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
                valid_patterns.push(pattern.to_string());
            }
            Err(error) => warnings.push(RiskWarning::new(
                RiskWarningKind::InvalidRiskPattern,
                format!("invalid risk path pattern ignored: {pattern}"),
                None,
                Some(pattern.to_string()),
                Some(error.to_string()),
            )),
        }
    }

    let set = builder
        .build()
        .expect("globset build should succeed after invalid patterns are skipped");
    (set, valid_patterns, warnings)
}

fn path_matches_name(path: &str, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| path == *name || path.ends_with(&format!("/{name}")))
}

impl RiskWarning {
    fn new(
        kind: RiskWarningKind,
        message: impl Into<String>,
        path: Option<String>,
        pattern: Option<String>,
        details: Option<String>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            path,
            pattern,
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_changed_files_and_deleted_files_from_git_diff() {
        let diff = "\
diff --git a/src/auth/session.rs b/src/auth/session.rs
index 1111111..2222222 100644
--- a/src/auth/session.rs
+++ b/src/auth/session.rs
@@ -1 +1 @@
-old
+new
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 3333333..0000000
--- a/old.txt
+++ /dev/null
";

        let files = changed_files_from_diff(diff);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/auth/session.rs");
        assert!(!files[0].deleted);
        assert_eq!(files[1].path, "old.txt");
        assert!(files[1].deleted);
    }

    #[test]
    fn generates_configured_and_built_in_warnings() {
        let diff = "\
diff --git a/src/auth/session.rs b/src/auth/session.rs
--- a/src/auth/session.rs
+++ b/src/auth/session.rs
diff --git a/Cargo.toml b/Cargo.toml
--- a/Cargo.toml
+++ b/Cargo.toml
diff --git a/pnpm-lock.yaml b/pnpm-lock.yaml
--- a/pnpm-lock.yaml
+++ b/pnpm-lock.yaml
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
";
        let config = RiskConfig {
            paths: vec!["src/auth/**".to_string()],
            large_diff_file_threshold: 2,
        };

        let warnings = analyze_diff_risk(diff, &config);

        assert!(has_kind(&warnings, RiskWarningKind::RiskPath));
        assert!(has_kind(&warnings, RiskWarningKind::DependencyManifest));
        assert!(has_kind(&warnings, RiskWarningKind::Lockfile));
        assert!(has_kind(&warnings, RiskWarningKind::DeletedFile));
        assert!(has_kind(&warnings, RiskWarningKind::LargeDiff));
    }

    fn has_kind(warnings: &[RiskWarning], kind: RiskWarningKind) -> bool {
        warnings.iter().any(|warning| warning.kind == kind)
    }
}
