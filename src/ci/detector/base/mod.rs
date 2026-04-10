use anyhow::Result;
use std::path::Path;

use crate::ci::parser::Pipeline;

/// Metadata extracted from a project
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// Detected project type
    pub project_type: ProjectType,
    /// Language/runtime version (e.g., "17" for JDK 17, "1.78" for Rust)
    pub language_version: Option<String>,
    /// Framework and version (e.g., "spring-boot 3.2.0")
    pub framework: Option<String>,
    /// Docker image to use
    pub image: String,
    /// Build command
    pub build_cmd: Vec<String>,
    /// Test command
    pub test_cmd: Vec<String>,
    /// Lint command (optional)
    pub lint_cmd: Option<Vec<String>>,
    /// Format check command (optional)
    pub fmt_cmd: Option<Vec<String>>,
    /// Context paths for auto_fix
    pub source_paths: Vec<String>,
    /// Config files relevant for auto_fix
    pub config_files: Vec<String>,
    /// Warnings to show the user
    pub warnings: Vec<String>,
    /// Detected quality/defect analysis plugins (e.g., "spotbugs", "pmd")
    pub quality_plugins: Vec<String>,
    /// Subdirectory where the project was detected (None if root)
    pub subdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectType {
    Maven,
    Gradle,
    Rust,
    Node,
    Python,
    Go,
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectType::Maven => write!(f, "Maven/Java"),
            ProjectType::Gradle => write!(f, "Gradle/Java"),
            ProjectType::Rust => write!(f, "Rust"),
            ProjectType::Node => write!(f, "Node.js"),
            ProjectType::Python => write!(f, "Python"),
            ProjectType::Go => write!(f, "Go"),
        }
    }
}

/// Strategy pattern trait for project detection
pub trait ProjectDetector {
    /// Check if this detector matches the given directory
    fn detect(&self, dir: &Path) -> bool;

    /// Analyze the project and extract metadata
    fn analyze(&self, dir: &Path) -> Result<ProjectInfo>;
}

/// All registered detectors, in priority order
fn all_detectors() -> Vec<Box<dyn ProjectDetector>> {
    vec![
        Box::new(super::maven::MavenDetector),
        Box::new(super::gradle::GradleDetector),
        Box::new(super::rust_project::RustDetector),
        Box::new(super::node::NodeDetector),
        Box::new(super::python::PythonDetector),
        Box::new(super::go::GoDetector),
    ]
}

/// Auto-detect project type and generate pipeline.
/// First tries the root directory, then scans immediate subdirectories.
pub fn detect_and_generate(dir: &Path) -> Result<(ProjectInfo, Pipeline)> {
    let detectors = all_detectors();

    // Try root directory first
    for detector in &detectors {
        if detector.detect(dir) {
            let mut info = detector.analyze(dir)?;
            info.subdir = None;
            let (pipeline, _step_defs) = crate::ci::pipeline_builder::generate_pipeline(&info);
            return Ok((info, pipeline));
        }
    }

    // Scan immediate subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let subpath = entry.path();
            if !subpath.is_dir() {
                continue;
            }
            // Skip hidden directories and common non-project dirs
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.')
                || name_str == "node_modules"
                || name_str == "target"
                || name_str == "dist"
                || name_str == "build"
            {
                continue;
            }
            for detector in &detectors {
                if detector.detect(&subpath) {
                    let mut info = detector.analyze(&subpath)?;
                    let subdir_name = name_str.to_string();
                    info = adapt_for_subdir(info, &subdir_name);
                    let (pipeline, _step_defs) =
                        crate::ci::pipeline_builder::generate_pipeline(&info);
                    return Ok((info, pipeline));
                }
            }
        }
    }

    anyhow::bail!(
        "Could not detect project type in '{}' or its subdirectories. Supported: Maven, Gradle, Rust, Node.js, Python, Go",
        dir.display()
    );
}

/// Adapt ProjectInfo for a project detected in a subdirectory.
/// Prefixes commands with `cd <subdir>`, and paths with `<subdir>/`.
fn adapt_for_subdir(mut info: ProjectInfo, subdir: &str) -> ProjectInfo {
    let prefix_cmd = |cmds: Vec<String>| -> Vec<String> {
        cmds.into_iter()
            .map(|cmd| format!("cd {} && {}", subdir, cmd))
            .collect()
    };
    let prefix_path = |paths: Vec<String>| -> Vec<String> {
        paths
            .into_iter()
            .map(|p| format!("{}/{}", subdir, p))
            .collect()
    };

    info.build_cmd = prefix_cmd(info.build_cmd);
    info.test_cmd = prefix_cmd(info.test_cmd);
    info.lint_cmd = info.lint_cmd.map(prefix_cmd);
    info.fmt_cmd = info.fmt_cmd.map(prefix_cmd);
    // Source paths point to the subdir root (original relative paths like src/main/java/
    // are internal to the subdir and would create wrong double-prefixed paths)
    info.source_paths = vec![format!("{}/", subdir)];
    info.config_files = prefix_path(info.config_files);
    info.subdir = Some(subdir.to_string());
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pipeline_basic() {
        let info = ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec![
                "rustup component add clippy 2>/dev/null; cargo clippy -- -D warnings".into(),
            ]),
            fmt_cmd: Some(vec![
                "rustup component add rustfmt 2>/dev/null; cargo fmt -- --check".into(),
            ]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        };
        let (pipeline, _step_defs) = crate::ci::pipeline_builder::generate_pipeline(&info);
        assert_eq!(pipeline.name, "rust-ci");
        // ping-pong + git-pull + RustStrategy: ping-pong, git-pull, build, clippy, test, fmt-check
        assert_eq!(pipeline.steps.len(), 6);
        assert_eq!(pipeline.steps[0].name, "ping-pong");
        assert_eq!(pipeline.steps[1].name, "git-pull");
        assert_eq!(pipeline.steps[2].name, "build");
        // build should depend on git-pull
        assert!(pipeline.steps[2]
            .depends_on
            .contains(&"git-pull".to_string()));
    }

    #[test]
    fn test_generate_pipeline_no_lint_or_fmt() {
        let info = ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some("1.22".into()),
            framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec!["go build ./...".into()],
            test_cmd: vec!["go test ./...".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec![".".into()],
            config_files: vec!["go.mod".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        };
        let (pipeline, _step_defs) = crate::ci::pipeline_builder::generate_pipeline(&info);
        // ping-pong + git-pull + GoStrategy: ping-pong, git-pull, build, vet, test (no lint, no fmt)
        assert_eq!(pipeline.steps.len(), 5);
        assert_eq!(pipeline.steps[0].name, "ping-pong");
        assert_eq!(pipeline.steps[1].name, "git-pull");
        assert_eq!(pipeline.steps[2].name, "build");
        assert_eq!(pipeline.steps[3].name, "vet");
        assert_eq!(pipeline.steps[4].name, "test");
    }
}
