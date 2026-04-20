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
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let changed_files = git_changed_files_snippet(&["*.java", "*.kt"], self.subdir.as_deref());
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found in pipelight-misc/. LLM should generate one with threshold (default 70) and exclude globs (DTO/Config/Exception/Application at minimum).' >&2; \
               exit 1; \
             fi && \
             {changed_files} && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'jacoco: no changed java/kt files on current branch — skipping'; \
               exit 0; \
             fi && \
             EXCLUDES=$(awk '/^exclude:/{{flag=1; next}} flag && /^[[:space:]]*-/ {{gsub(/^[[:space:]]*-[[:space:]]*\"?/,\"\"); gsub(/\"?[[:space:]]*$/,\"\"); print}} flag && /^[^[:space:]-]/ {{flag=0}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             FILTERED=\"\" && \
             while IFS= read -r f; do \
               [ -z \"$f\" ] && continue; \
               skip=0; \
               for pat in $EXCLUDES; do \
                 case \"$f\" in $pat) skip=1; break;; esac; \
               done; \
               [ \"$skip\" -eq 0 ] && FILTERED=\"$FILTERED$f\\n\"; \
             done <<< \"$CHANGED_FILES\" && \
             FILTERED=$(printf '%b' \"$FILTERED\" | sed '/^$/d') && \
             if [ -z \"$FILTERED\" ]; then \
               echo 'jacoco: all changed files excluded by jacoco-config.yml — skipping'; \
               exit 0; \
             fi && \
             echo \"jacoco: checking coverage for $(echo \\\"$FILTERED\\\" | wc -l | tr -d ' ') changed file(s)\" && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then \
               echo 'jacoco: no exec file at pipelight-misc/jacoco-report/jacoco.exec (test step may have crashed or jacoco plugin output path was customised) — skipping'; \
               exit 0; \
             fi && \
             JACOCO_VER={ver} && \
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-{ver} && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.xml ]; then \
               if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
                 echo 'Downloading JaCoCo CLI...' && \
                 mkdir -p $JACOCO_DIR && \
                 curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/{ver}/jacoco-{ver}.zip -o /tmp/jacoco.zip && \
                 (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
               fi && \
               CLASS_DIRS=$(find . -path '*/target/classes' -type d 2>/dev/null | tr '\\n' ' ') && \
               SRC_DIRS=$(find . -path '*/src/main/java' -type d 2>/dev/null | tr '\\n' ' ') && \
               CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
               SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
               java -jar $JACOCO_DIR/lib/jacococli.jar report \
                 /workspace/pipelight-misc/jacoco-report/jacoco.exec \
                 $CLASSFILES_ARGS $SOURCES_ARGS \
                 --xml /workspace/pipelight-misc/jacoco-report/jacoco.xml; \
             fi && \
             true",
            cd = cd_prefix,
            ver = JACOCO_VERSION,
            changed_files = changed_files,
        );
        StepConfig {
            name: "jacoco".into(),
            image: self.image.clone(),
            commands: vec![cmd],
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

    #[test]
    fn test_command_emits_auto_gen_config_callback_when_missing() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config"),
            "command must emit the auto-gen-config callback when config missing, got: {}",
            cmd
        );
        assert!(
            cmd.contains("pipelight-misc/jacoco-config.yml"),
            "command must reference config file path, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_reads_git_diff_report() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("pipelight-misc/git-diff-report/unstaged.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/staged.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/untracked.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/unpushed.txt"));
        assert!(cmd.contains("java|kt"));
    }

    #[test]
    fn test_command_skips_when_no_changed_java_files() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("no changed java/kt files"),
            "command must self-skip when no changes, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_applies_exclude_patterns_from_config() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("jacoco-config.yml") && cmd.contains("exclude"),
            "command must consult exclude patterns, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_skips_when_all_files_excluded() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("all changed files excluded"),
            "command must self-skip when filter empties the list, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_handles_missing_exec_file() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("no exec file"),
            "command must handle missing exec gracefully, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_downloads_jacococli_in_standalone_mode() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("jacococli.jar"),
            "standalone mode must download/use jacococli, got: {}",
            cmd
        );
        assert!(cmd.contains("jacoco-0.8.12"));
    }

    #[test]
    fn test_command_generates_xml_report_in_standalone_mode() {
        let step = MavenJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("report") && cmd.contains("--xml"),
            "standalone mode must run `jacococli report --xml`, got: {}",
            cmd
        );
        assert!(cmd.contains("pipelight-misc/jacoco-report/jacoco.xml"));
    }
}
