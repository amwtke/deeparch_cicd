pub mod maven;
pub mod gradle;
pub mod rust_project;
pub mod node;
pub mod python;
pub mod go;

use anyhow::Result;
use std::path::Path;

use crate::pipeline::Pipeline;

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
        Box::new(maven::MavenDetector),
        Box::new(gradle::GradleDetector),
        Box::new(rust_project::RustDetector),
        Box::new(node::NodeDetector),
        Box::new(python::PythonDetector),
        Box::new(go::GoDetector),
    ]
}

/// Auto-detect project type and generate pipeline
pub fn detect_and_generate(dir: &Path) -> Result<(ProjectInfo, Pipeline)> {
    let detectors = all_detectors();

    for detector in &detectors {
        if detector.detect(dir) {
            let info = detector.analyze(dir)?;
            let pipeline = crate::strategy::generate_pipeline(&info);
            return Ok((info, pipeline));
        }
    }

    anyhow::bail!(
        "Could not detect project type in '{}'. Supported: Maven, Gradle, Rust, Node.js, Python, Go",
        dir.display()
    );
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
            lint_cmd: Some(vec!["cargo clippy -- -D warnings".into()]),
            fmt_cmd: Some(vec!["cargo fmt -- --check".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
        };
        let pipeline = crate::strategy::generate_pipeline(&info);
        assert_eq!(pipeline.name, "rust-ci");
        // RustStrategy: build, clippy, test, fmt-check
        assert_eq!(pipeline.steps.len(), 4);
        assert_eq!(pipeline.steps[0].name, "build");
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
        };
        let pipeline = crate::strategy::generate_pipeline(&info);
        // GoStrategy: build, vet, test (no lint, no fmt)
        assert_eq!(pipeline.steps.len(), 3);
        assert_eq!(pipeline.steps[0].name, "build");
        assert_eq!(pipeline.steps[1].name, "vet");
        assert_eq!(pipeline.steps[2].name, "test");
    }
}
