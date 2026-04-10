pub mod base;
pub mod go;
pub mod gradle;
pub mod maven;
pub mod node;
pub mod python;
pub mod rust_lang;
pub mod test_parser;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::ExceptionMapping;
use crate::ci::detector::{ProjectInfo, ProjectType};
use crate::ci::parser::{OnFailure, Pipeline, Step};

/// Data carrier for a single pipeline step (renamed from old StepDef struct).
#[derive(Debug, Clone)]
pub struct StepConfig {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,
    pub on_failure: Option<OnFailure>,
    pub allow_failure: bool,
    pub volumes: Vec<String>,
    pub local: bool,
}

impl Default for StepConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            commands: vec![],
            depends_on: vec![],
            workdir: "/workspace".into(),
            on_failure: None,
            allow_failure: false,
            volumes: vec![],
            local: false,
        }
    }
}

impl From<StepConfig> for Step {
    fn from(sc: StepConfig) -> Self {
        Step {
            name: sc.name,
            image: sc.image,
            commands: sc.commands,
            depends_on: sc.depends_on,
            workdir: sc.workdir,
            on_failure: sc.on_failure,
            allow_failure: sc.allow_failure,
            volumes: sc.volumes,
            local: sc.local,
            env: HashMap::new(),
            condition: None,
        }
    }
}

/// Each pipeline step is now a trait object, carrying both its config
/// and the ability to produce human-readable reports from execution output.
pub trait StepDef: Send + Sync {
    /// Return the step configuration (image, commands, dependencies, etc.)
    fn config(&self) -> StepConfig;

    /// Produce a one-line human-readable summary of step execution.
    #[allow(dead_code)]
    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String;

    /// Write a timestamped log file and return its path.
    /// Default implementation delegates to `write_step_report`.
    #[allow(dead_code)]
    fn output_report_path(&self, misc_dir: &Path, stdout: &str, stderr: &str) -> PathBuf {
        let cfg = self.config();
        write_step_report(misc_dir, &cfg.name, stdout, stderr)
    }

    /// Return the exception-to-command mapping for this step.
    /// Default: empty mapping with Abort fallback (all failures are fatal).
    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }

    /// Analyze execution output to identify the exception key.
    /// Called as priority 2 in resolve chain (after stderr marker).
    /// Default: None (no Rust-side analysis).
    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        None
    }
}

/// Language-specific pipeline generation strategy.
pub trait PipelineStrategy {
    /// Produce the ordered list of step definitions for this project type.
    fn steps(&self, info: &ProjectInfo) -> Vec<Box<dyn StepDef>>;

    /// Return the pipeline name (e.g. "maven-ci", "rust-ci").
    fn pipeline_name(&self, info: &ProjectInfo) -> String;

    /// Parse step output into a human-readable summary line.
    /// Default: delegates to BaseStrategy for common steps.
    fn output_report_str(
        &self,
        step_name: &str,
        success: bool,
        stdout: &str,
        stderr: &str,
    ) -> String {
        base::BaseStrategy::default_report_str(step_name, success, stdout, stderr)
    }

    /// Parse test step output into structured TestSummary (backward compat for JSON output).
    fn parse_test_output(&self, _output: &str) -> Option<test_parser::TestSummary> {
        None
    }
}

fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    match project_type {
        ProjectType::Maven => Box::new(maven::MavenStrategy),
        ProjectType::Gradle => Box::new(gradle::GradleStrategy),
        ProjectType::Rust => Box::new(rust_lang::RustStrategy),
        ProjectType::Node => Box::new(node::NodeStrategy),
        ProjectType::Python => Box::new(python::PythonStrategy),
        ProjectType::Go => Box::new(go::GoStrategy),
    }
}

/// Get strategy by pipeline name prefix (for parsing output after execution).
/// Returns None if no matching strategy is found.
pub fn strategy_for_pipeline(pipeline: &Pipeline) -> Option<Box<dyn PipelineStrategy>> {
    let name = &pipeline.name;
    if name.starts_with("maven") {
        Some(Box::new(maven::MavenStrategy))
    } else if name.starts_with("gradle") {
        Some(Box::new(gradle::GradleStrategy))
    } else if name.starts_with("rust") {
        Some(Box::new(rust_lang::RustStrategy))
    } else if name.starts_with("node") {
        Some(Box::new(node::NodeStrategy))
    } else if name.starts_with("python") {
        Some(Box::new(python::PythonStrategy))
    } else if name.starts_with("go") {
        Some(Box::new(go::GoStrategy))
    } else {
        None
    }
}

/// Generate a Pipeline from ProjectInfo using the strategy system.
/// Returns both the Pipeline and the trait-object step definitions
/// (so callers can use output_report_str / output_report_path later).
///
/// A fixed `git-pull` step is always prepended, and all root steps
/// (those with no dependencies) are wired to depend on it.
pub fn generate_pipeline(info: &ProjectInfo) -> (Pipeline, Vec<Box<dyn StepDef>>) {
    let strategy = strategy_for(&info.project_type);
    let mut step_defs = strategy.steps(info);
    let name = strategy.pipeline_name(info);

    // Prepend git-pull and wire root steps to depend on it
    let git_pull = base::GitPullStep::new();
    let git_pull_name = {
        let cfg = git_pull.config();
        cfg.name.clone()
    };

    for sd in &mut step_defs {
        let cfg = sd.config();
        if cfg.depends_on.is_empty() {
            // We need to rebuild with the dependency added.
            // Strategies must ensure root steps get wired; handled below via StepConfig.
        }
    }

    // Collect configs, wiring root steps to depend on git-pull
    let git_pull_cfg = git_pull.config();
    let mut all_configs: Vec<StepConfig> = vec![git_pull_cfg];

    for sd in &step_defs {
        let mut cfg = sd.config();
        if cfg.depends_on.is_empty() {
            cfg.depends_on.push(git_pull_name.clone());
        }
        all_configs.push(cfg);
    }

    // Build the full step def list with git-pull at the front
    let mut all_step_defs: Vec<Box<dyn StepDef>> = vec![Box::new(git_pull)];
    all_step_defs.extend(step_defs);

    let pipeline = Pipeline {
        name,
        git_credentials: Some(crate::ci::parser::GitCredentials {
            username: "your_username".to_string(),
            password: "your_token_or_password".to_string(),
        }),
        env: HashMap::new(),
        steps: all_configs.into_iter().map(|sc| sc.into()).collect(),
    };

    (pipeline, all_step_defs)
}

/// Write step stdout+stderr to pipelight-misc/{step_name}-{timestamp}.log.
/// Always writes (success or failure). Returns the written file path.
pub fn write_step_report(misc_dir: &Path, step_name: &str, stdout: &str, stderr: &str) -> PathBuf {
    let timestamp = chrono::Local::now().format("%Y%m%dT%H%M%S");
    let filename = format!("{}-{}.log", step_name, timestamp);
    let log_path = misc_dir.join(&filename);

    let mut content = String::new();
    if !stdout.is_empty() {
        content.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(stderr);
    }

    if let Err(e) = std::fs::write(&log_path, &content) {
        tracing::warn!(
            "Failed to write step report to {}: {}",
            log_path.display(),
            e
        );
    }

    log_path
}

/// Count lines in output that match any of the given patterns.
pub fn count_pattern(output: &str, patterns: &[&str]) -> usize {
    output
        .lines()
        .filter(|line| patterns.iter().any(|p| line.contains(p)))
        .count()
}
