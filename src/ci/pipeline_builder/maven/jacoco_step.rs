use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{JacocoMode, JACOCO_VERSION};
use crate::ci::pipeline_builder::{git_changed_files_snippet, StepConfig, StepDef};

/// Incremental JaCoCo coverage check (tag = "non-full").
///
/// Reads `pipelight-misc/jacoco-report/jacoco.exec` (populated by the
/// `JacocoAgentTestStep`-wrapped test step), generates a JaCoCo XML report
/// (standalone/plugin modes differ on whether the XML is already produced),
/// filters sourcefile entries to the git-diff working-branch changes
/// (minus the exclude patterns in `jacoco-config.yml`), and fails if any
/// changed file's LINE coverage is below the threshold. Failures fire an
/// `AutoFix` callback so the LLM can add unit tests and retry.
pub struct MavenJacocoStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    mode: JacocoMode,
}

impl MavenJacocoStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for MavenJacocoStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec!["true".into()], // placeholder; filled in next tasks
            depends_on: vec!["test".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        // Filled in Task 14.
        ExceptionMapping::new(CallbackCommand::RuntimeError)
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        None
    }

    fn output_report_str(&self, success: bool, _stdout: &str, _stderr: &str) -> String {
        if success {
            "jacoco: ok".into()
        } else {
            "jacoco: failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::ProjectType;

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile -q".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_basic_step_config() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco");
        assert_eq!(cfg.depends_on, vec!["test".to_string()]);
        assert_eq!(cfg.tag, "non-full");
        assert!(!cfg.allow_failure);
        assert!(cfg.active);
    }
}
