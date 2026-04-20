use crate::ci::callback::exception::ExceptionMapping;
use crate::ci::pipeline_builder::{StepConfig, StepDef};
use regex::Regex;

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
        let mut cfg = self.inner.config();
        match self.mode {
            JacocoMode::None => {}
            JacocoMode::Standalone => {
                let is_gradle = cfg.image.contains("gradle");
                let agent_env = if is_gradle {
                    "JAVA_TOOL_OPTIONS"
                } else {
                    "MAVEN_OPTS"
                };
                let agent_jar = format!(
                    "$HOME/.pipelight/cache/jacoco-{ver}/lib/jacocoagent.jar",
                    ver = JACOCO_VERSION
                );
                let destfile = "/workspace/pipelight-misc/jacoco-report/jacoco.exec";
                let prepare = format!(
                    "JACOCO_CACHE=$HOME/.pipelight/cache && \
                     JACOCO_DIR=$JACOCO_CACHE/jacoco-{ver} && \
                     if [ ! -f {agent} ]; then \
                       echo 'Downloading JaCoCo {ver} ...' && \
                       mkdir -p $JACOCO_DIR && \
                       curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/{ver}/jacoco-{ver}.zip \
                         -o /tmp/jacoco.zip && \
                       (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && \
                       rm -f /tmp/jacoco.zip; \
                     fi && \
                     mkdir -p /workspace/pipelight-misc/jacoco-report && \
                     export {env}=\"-javaagent:{agent}=destfile={destfile},append=false\"",
                    ver = JACOCO_VERSION,
                    agent = agent_jar,
                    env = agent_env,
                    destfile = destfile,
                );
                let joined_original = cfg.commands.join(" && ");
                cfg.commands = vec![format!("{prepare} && {joined_original}")];
            }
            JacocoMode::MavenPlugin => {
                let destfile = "/workspace/pipelight-misc/jacoco-report/jacoco.exec";
                // Word-boundary regex avoids false positives like `mvn test-compile`.
                // Match "mvn test" only when followed by space, dash from flag, or end-of-string.
                let mvn_test_re = Regex::new(r"\bmvn\s+test(\s|$)").unwrap();
                let any_rewritten = cfg.commands.iter().any(|c| mvn_test_re.is_match(c));
                if any_rewritten {
                    let replacement = format!("mvn jacoco:prepare-agent test$1");
                    let rewritten: Vec<String> = cfg
                        .commands
                        .iter()
                        .map(|c| {
                            let replaced = mvn_test_re
                                .replace_all(c, replacement.as_str())
                                .into_owned();
                            // Now inject the destFile property at the end of "test"
                            replaced.replace(
                                "mvn jacoco:prepare-agent test",
                                &format!(
                                    "mvn jacoco:prepare-agent test -Djacoco.destFile={destfile}"
                                ),
                            )
                        })
                        .collect();
                    let joined = rewritten.join(" && ");
                    cfg.commands = vec![format!(
                        "mkdir -p /workspace/pipelight-misc/jacoco-report && {joined}"
                    )];
                }
                // else: nothing to rewrite — leave commands untouched.
            }
            JacocoMode::GradlePlugin => {
                let rewritten: Vec<String> = cfg
                    .commands
                    .iter()
                    .map(|c| {
                        if c.contains("./gradlew") && !c.contains("jacocoTestReport") {
                            format!("{c} jacocoTestReport")
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                let joined = rewritten.join(" && ");
                let copy_cmd = "mkdir -p /workspace/pipelight-misc/jacoco-report && \
                     cp build/jacoco/test.exec /workspace/pipelight-misc/jacoco-report/jacoco.exec 2>/dev/null || true && \
                     cp build/reports/jacoco/test/jacocoTestReport.xml /workspace/pipelight-misc/jacoco-report/jacoco.xml 2>/dev/null || true";
                cfg.commands = vec![format!("{joined} && {copy_cmd}")];
            }
        }
        cfg
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

    #[test]
    fn test_standalone_mode_maven_injects_javaagent() {
        let inner = TestStep::new(&make_info("mvn test", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::Standalone);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacoco-0.8.12"),
            "expected pinned JaCoCo version in command, got: {}",
            combined
        );
        assert!(
            combined.contains("MAVEN_OPTS"),
            "expected MAVEN_OPTS env var, got: {}",
            combined
        );
        assert!(
            combined.contains("-javaagent:"),
            "expected -javaagent injection, got: {}",
            combined
        );
        assert!(
            combined.contains("destfile=/workspace/pipelight-misc/jacoco-report/jacoco.exec"),
            "expected destfile in pipelight-misc, got: {}",
            combined
        );
        assert!(combined.contains("mvn test"));
    }

    #[test]
    fn test_standalone_mode_gradle_injects_java_tool_options() {
        let inner = TestStep::new(&make_info("./gradlew test", "gradle:8-jdk17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::Standalone);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("JAVA_TOOL_OPTIONS"),
            "expected JAVA_TOOL_OPTIONS env var for Gradle, got: {}",
            combined
        );
        assert!(combined.contains("-javaagent:"));
        assert!(combined.contains("./gradlew test"));
    }

    #[test]
    fn test_standalone_mode_downloads_agent_if_missing() {
        let inner = TestStep::new(&make_info("mvn test", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::Standalone);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacoco-0.8.12.zip") || combined.contains("jacoco-0.8.12-bin"),
            "expected agent download logic, got: {}",
            combined
        );
        assert!(
            combined.contains("~/.pipelight/cache") || combined.contains("$HOME/.pipelight/cache")
        );
    }

    #[test]
    fn test_maven_plugin_mode_injects_prepare_agent() {
        let inner = TestStep::new(&make_info(
            "mvn test --fail-at-end",
            "maven:3.9-eclipse-temurin-17",
        ));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::MavenPlugin);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacoco:prepare-agent"),
            "expected jacoco:prepare-agent goal inserted, got: {}",
            combined
        );
        assert!(
            combined
                .contains("-Djacoco.destFile=/workspace/pipelight-misc/jacoco-report/jacoco.exec"),
            "expected destFile system property, got: {}",
            combined
        );
        assert!(combined.contains("--fail-at-end"));
    }

    #[test]
    fn test_maven_plugin_mode_leaves_non_mvn_untouched() {
        let inner = TestStep::new(&make_info("echo hi", "maven:3.9-eclipse-temurin-17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::MavenPlugin);
        let cfg = decorator.config();
        assert_eq!(cfg.commands, vec!["echo hi".to_string()]);
    }

    #[test]
    fn test_maven_plugin_mode_does_not_match_test_compile() {
        // False-positive guard: "mvn test-compile" must not be rewritten.
        let inner = TestStep::new(&make_info(
            "mvn test-compile",
            "maven:3.9-eclipse-temurin-17",
        ));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::MavenPlugin);
        let cfg = decorator.config();
        assert_eq!(cfg.commands, vec!["mvn test-compile".to_string()]);
    }

    #[test]
    fn test_gradle_plugin_mode_appends_jacoco_test_report() {
        let inner = TestStep::new(&make_info("./gradlew test --continue", "gradle:8-jdk17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::GradlePlugin);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("jacocoTestReport"),
            "expected jacocoTestReport task appended, got: {}",
            combined
        );
        assert!(combined.contains("--continue"), "must preserve --continue");
    }

    #[test]
    fn test_gradle_plugin_mode_copies_outputs_to_pipelight_misc() {
        let inner = TestStep::new(&make_info("./gradlew test", "gradle:8-jdk17"));
        let decorator = JacocoAgentTestStep::new(Box::new(inner), JacocoMode::GradlePlugin);
        let cfg = decorator.config();
        let combined = cfg.commands.join(" && ");
        assert!(
            combined.contains("cp build/jacoco/test.exec"),
            "expected cp fallback for exec, got: {}",
            combined
        );
        assert!(
            combined.contains("jacocoTestReport.xml"),
            "expected cp fallback for xml, got: {}",
            combined
        );
        assert!(
            combined.contains("pipelight-misc/jacoco-report"),
            "expected destination path, got: {}",
            combined
        );
        assert!(
            combined.contains("|| true"),
            "copy must be tolerant of missing files (|| true)"
        );
    }
}
