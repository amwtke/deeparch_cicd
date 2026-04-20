use crate::ci::callback::exception::ExceptionMapping;
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// JaCoCo execution mode selected by the strategy at pipeline-build time.
///
/// - `None` — no JaCoCo wiring; decorator is a no-op.
/// - `Standalone` — download the JaCoCo agent into the pipelight cache and
///   inject it via `MAVEN_OPTS` / `JAVA_TOOL_OPTIONS` so the test JVM
///   instruments classes at runtime and writes `jacoco.exec`.
/// - `MavenPlugin` — rewrite `mvn test` to `mvn jacoco:prepare-agent test ...`
///   so the project's own jacoco-maven-plugin handles instrumentation.
/// - `GradlePlugin` — append `jacocoTestReport` to the Gradle command; copy
///   Gradle's default outputs into `pipelight-misc/jacoco-report/` so the
///   downstream jacoco step finds them in a predictable location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JacocoMode {
    None,
    Standalone,
    MavenPlugin,
    GradlePlugin,
}

/// Pinned JaCoCo version. Supports Java 5–22 bytecode instrumentation and
/// requires only JRE 1.8+ to run jacococli, covering the user's Java 8/17 span.
pub const JACOCO_VERSION: &str = "0.8.12";

/// Decorator wrapping a `TestStep` to inject the JaCoCo agent or switch to
/// the project's JaCoCo plugin. Non-Java pipelines pass `JacocoMode::None`
/// and the decorator behaves as a no-op (all trait methods forward to inner).
pub struct JacocoAgentTestStep {
    inner: Box<dyn StepDef>,
    mode: JacocoMode,
}

impl JacocoAgentTestStep {
    pub fn new(inner: Box<dyn StepDef>, mode: JacocoMode) -> Self {
        Self { inner, mode }
    }
}

impl StepDef for JacocoAgentTestStep {
    fn config(&self) -> StepConfig {
        // Implemented in the next tasks.
        self.inner.config()
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        self.inner.exception_mapping()
    }

    fn match_exception(&self, exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        self.inner.match_exception(exit_code, stdout, stderr)
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        self.inner.output_report_str(success, stdout, stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};
    use crate::ci::pipeline_builder::base::TestStep;

    fn make_info(test_cmd: &str, image: &str) -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: image.into(),
            build_cmd: vec!["mvn compile -q".into()],
            test_cmd: vec![test_cmd.into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_mode_none_passes_commands_through() {
        let inner = TestStep::new(&make_info("mvn test", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::None);
        let cfg = decorator.config();
        assert_eq!(cfg.commands, vec!["mvn test".to_string()]);
    }
}
