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
use crate::ci::parser::{Pipeline, Step};

/// Data carrier for a single pipeline step (renamed from old StepDef struct).
#[derive(Debug, Clone)]
pub struct StepConfig {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,
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
            on_failure: None,
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
        ExceptionMapping::new(CallbackCommand::RuntimeError)
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

/// Reconstruct StepDef trait objects for a pipeline loaded from YAML.
/// Returns a map from step name to StepDef. Only works if the pipeline
/// was generated by pipelight (name matches a known strategy).
/// Returns None if no matching strategy is found.
///
/// Note: The reconstructed ProjectInfo is approximate — fields like
/// source_paths and config_files are inferred from on_failure.context_paths
/// and project type conventions, not from the original detector output.
pub fn step_defs_for_pipeline(pipeline: &Pipeline) -> Option<HashMap<String, Box<dyn StepDef>>> {
    let strategy = strategy_for_pipeline(pipeline)?;

    // We need ProjectInfo to build step defs, but we don't have it when loading from YAML.
    // Reconstruct a minimal ProjectInfo from the pipeline steps.
    let first_docker_step = pipeline.steps.iter().find(|s| !s.image.is_empty())?;
    let source_paths: Vec<String> = pipeline
        .steps
        .iter()
        .filter_map(|s| s.on_failure.as_ref())
        .flat_map(|of| of.context_paths.iter().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let info = crate::ci::detector::ProjectInfo {
        project_type: match pipeline.name.as_str() {
            n if n.starts_with("maven") => crate::ci::detector::ProjectType::Maven,
            n if n.starts_with("gradle") => crate::ci::detector::ProjectType::Gradle,
            n if n.starts_with("rust") => crate::ci::detector::ProjectType::Rust,
            n if n.starts_with("node") => crate::ci::detector::ProjectType::Node,
            n if n.starts_with("python") => crate::ci::detector::ProjectType::Python,
            n if n.starts_with("go") => crate::ci::detector::ProjectType::Go,
            _ => return None,
        },
        language_version: None,
        framework: None,
        image: first_docker_step.image.clone(),
        build_cmd: pipeline
            .get_step("build")
            .map(|s| s.commands.clone())
            .unwrap_or_default(),
        test_cmd: pipeline
            .get_step("test")
            .map(|s| s.commands.clone())
            .unwrap_or_default(),
        lint_cmd: pipeline
            .get_step("clippy")
            .or_else(|| pipeline.get_step("lint"))
            .map(|s| s.commands.clone()),
        fmt_cmd: pipeline.get_step("fmt-check").map(|s| s.commands.clone()),
        source_paths,
        config_files: match pipeline.name.as_str() {
            n if n.starts_with("rust") => vec!["Cargo.toml".into()],
            n if n.starts_with("maven") => vec!["pom.xml".into()],
            n if n.starts_with("gradle") => vec!["build.gradle".into()],
            n if n.starts_with("node") => vec!["package.json".into()],
            n if n.starts_with("python") => vec!["pyproject.toml".into()],
            n if n.starts_with("go") => vec!["go.mod".into()],
            _ => vec![],
        },
        warnings: vec![],
        quality_plugins: vec![],
        subdir: None,
    };

    let step_defs = strategy.steps(&info);
    let git_pull = base::GitPullStep::new();

    let mut map: HashMap<String, Box<dyn StepDef>> = HashMap::new();
    let ping_pong = base::PingPongStep::new();
    map.insert(ping_pong.config().name, Box::new(ping_pong));
    map.insert(git_pull.config().name, Box::new(git_pull));
    for sd in step_defs {
        map.insert(sd.config().name, sd);
    }
    Some(map)
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

    // Prepend ping-pong (first) → git-pull (second), wire dependencies
    let ping_pong = base::PingPongStep::new();
    let ping_pong_name = ping_pong.config().name.clone();

    let git_pull = base::GitPullStep::new();
    let git_pull_name = git_pull.config().name.clone();

    for sd in &mut step_defs {
        let cfg = sd.config();
        if cfg.depends_on.is_empty() {
            // We need to rebuild with the dependency added.
            // Strategies must ensure root steps get wired; handled below via StepConfig.
        }
    }

    // Collect configs: ping-pong first, then git-pull (depends on ping-pong),
    // then strategy steps (root steps depend on git-pull)
    let ping_pong_cfg = ping_pong.config();

    let mut git_pull_cfg = git_pull.config();
    git_pull_cfg.depends_on.push(ping_pong_name.clone());

    let mut all_configs: Vec<StepConfig> = vec![ping_pong_cfg, git_pull_cfg];

    for sd in &step_defs {
        let mut cfg = sd.config();
        if cfg.depends_on.is_empty() {
            cfg.depends_on.push(git_pull_name.clone());
        }
        all_configs.push(cfg);
    }

    // Build the full step def list with ping-pong and git-pull at the front
    let mut all_step_defs: Vec<Box<dyn StepDef>> =
        vec![Box::new(ping_pong), Box::new(git_pull)];
    all_step_defs.extend(step_defs);

    // Convert configs to Steps, then attach on_failure from exception_mapping
    let mut steps: Vec<Step> = all_configs.into_iter().map(|sc| sc.into()).collect();

    // Attach on_failure from each StepDef's exception_mapping
    for (step, sd) in steps.iter_mut().zip(all_step_defs.iter()) {
        step.on_failure = Some(sd.exception_mapping().to_on_failure());
    }

    let pipeline = Pipeline {
        name,
        git_credentials: Some(crate::ci::parser::GitCredentials {
            username: "your_username".to_string(),
            password: "your_token_or_password".to_string(),
        }),
        env: HashMap::new(),
        steps,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::command::CallbackCommand;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn make_rust_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("2021".into()),
            framework: None,
            image: "rust:latest".into(),
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
        }
    }

    #[test]
    fn test_generate_pipeline_has_on_failure() {
        let info = make_rust_info();
        let (pipeline, _step_defs) = generate_pipeline(&info);

        // ping-pong: Ping, 9 retries (first step in pipeline)
        let ping_pong = pipeline.get_step("ping-pong").unwrap();
        let of = ping_pong
            .on_failure
            .as_ref()
            .expect("ping-pong should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::Ping);
        assert_eq!(of.max_retries, 9);
        assert!(ping_pong.depends_on.is_empty());

        // git-pull: GitFail, no retries (depends on ping-pong)
        let git_pull = pipeline.get_step("git-pull").unwrap();
        assert!(git_pull.depends_on.contains(&"ping-pong".to_string()));
        let of = git_pull
            .on_failure
            .as_ref()
            .expect("git-pull should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::GitFail);
        assert_eq!(of.max_retries, 0);

        // build: AutoFix, 3 retries
        let build = pipeline.get_step("build").unwrap();
        let of = build
            .on_failure
            .as_ref()
            .expect("build should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 3);
        assert!(of.context_paths.contains(&"src/".to_string()));
        assert!(of.context_paths.contains(&"Cargo.toml".to_string()));

        // test: Abort, no retries
        let test = pipeline.get_step("test").unwrap();
        let of = test
            .on_failure
            .as_ref()
            .expect("test should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::Abort);
        assert_eq!(of.max_retries, 0);

        // fmt-check: AutoFix, 1 retry
        let fmt = pipeline.get_step("fmt-check").unwrap();
        let of = fmt
            .on_failure
            .as_ref()
            .expect("fmt-check should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 1);
        assert!(of.context_paths.contains(&"src/".into()));

        // clippy: AutoFix, 2 retries
        let clippy = pipeline.get_step("clippy").unwrap();
        let of = clippy
            .on_failure
            .as_ref()
            .expect("clippy should have on_failure");
        assert_eq!(of.callback_command, CallbackCommand::AutoFix);
        assert_eq!(of.max_retries, 2);
    }

    #[test]
    fn test_step_defs_for_pipeline_rust() {
        let info = make_rust_info();
        let (pipeline, _) = generate_pipeline(&info);
        let defs = step_defs_for_pipeline(&pipeline).expect("should find rust strategy");
        assert!(defs.contains_key("ping-pong"));
        assert!(defs.contains_key("git-pull"));
        assert!(defs.contains_key("build"));
        assert!(defs.contains_key("test"));
        assert!(defs.contains_key("clippy"));
        assert!(defs.contains_key("fmt-check"));
    }
}
