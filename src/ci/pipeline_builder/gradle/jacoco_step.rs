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
pub struct GradleJacocoStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    mode: JacocoMode,
}

impl GradleJacocoStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for GradleJacocoStep {
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
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-{ver} && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.xml ]; then \
               if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
                 echo 'Downloading JaCoCo CLI...' && \
                 mkdir -p $JACOCO_DIR && \
                 curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/{ver}/jacoco-{ver}.zip -o /tmp/jacoco.zip && \
                 (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
               fi && \
               CLASS_DIRS=$(find . \\( -path '*/build/classes/java/main' -o -path '*/build/classes/kotlin/main' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
               SRC_DIRS=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
               CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
               SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
               java -jar $JACOCO_DIR/lib/jacococli.jar report \
                 /workspace/pipelight-misc/jacoco-report/jacoco.exec \
                 $CLASSFILES_ARGS $SOURCES_ARGS \
                 --xml /workspace/pipelight-misc/jacoco-report/jacoco.xml; \
             fi && \
             THRESHOLD=$(awk '/^threshold:/{{print $2; exit}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             THRESHOLD=${{THRESHOLD:-70}} && \
             REPORT=/workspace/pipelight-misc/jacoco-report && \
             : > $REPORT/jacoco-summary.txt && \
             : > $REPORT/uncovered.txt && \
             : > $REPORT/threshold-fail.txt && \
             awk -v threshold=\"$THRESHOLD\" \
                 -v summary=\"$REPORT/jacoco-summary.txt\" \
                 -v failed=\"$REPORT/threshold-fail.txt\" \
                 -v uncovered=\"$REPORT/uncovered.txt\" \
                 -v filtered_list=\"$FILTERED\" '\
               BEGIN {{ \
                 n=split(filtered_list, files, \"\\n\"); \
                 for (i=1;i<=n;i++) {{ if (files[i]!=\"\") keep[files[i]]=1 }}; \
                 IGNORECASE=1 \
               }} \
               /<package/ {{ match($0, /name=\"[^\"]*\"/); pkg=substr($0, RSTART+6, RLENGTH-7) }} \
               /<sourcefile/ {{ match($0, /name=\"[^\"]*\"/); current=substr($0, RSTART+6, RLENGTH-7); have_line=0 }} \
               /<line.*mi=/ && current!=\"\" && have_line==0 {{ \
                 if (match($0, /nr=\"[0-9]+\"/)) {{ nr=substr($0, RSTART+4, RLENGTH-5); \
                   if (match($0, /mi=\"[0-9]+\"/)) {{ mi=substr($0, RSTART+4, RLENGTH-5); if (mi+0>0) uncov[current]=uncov[current] nr \",\" }} \
                 }} \
               }} \
               /<counter type=\"LINE\"/ && current!=\"\" {{ \
                 if (have_line==1) next; have_line=1; \
                 match($0,/missed=\"[0-9]+\"/); m=substr($0,RSTART+8,RLENGTH-9)+0; \
                 match($0,/covered=\"[0-9]+\"/); c=substr($0,RSTART+9,RLENGTH-10)+0; \
                 t=m+c; pct=(t==0)?100:int((c*1000)/t)/10; \
                 rel=(pkg==\"\")?current:pkg\"/\"current; \
                 matched=\"\"; for (f in keep) if (index(f, rel)>0) {{ matched=f; break }}; \
                 if (matched!=\"\") {{ \
                   printf(\"%s %.1f%%\\n\", matched, pct) >> summary; \
                   if (pct<threshold) {{ \
                     printf(\"%s %.1f%%\\n\", matched, pct) >> failed; \
                     printf(\"%s: missed lines %s\\n\", matched, uncov[current]) >> uncovered; \
                   }} \
                 }} \
                 current=\"\"; \
               }} \
             ' $REPORT/jacoco.xml && \
             FAIL_COUNT=$(wc -l < $REPORT/threshold-fail.txt | tr -d ' ') && \
             echo \"\" && \
             echo \"JaCoCo Total: $FAIL_COUNT files below $THRESHOLD%\" && \
             if [ \"$FAIL_COUNT\" -gt 0 ]; then \
               echo \"\" && echo \"=== Files Below Threshold ===\" && cat $REPORT/threshold-fail.txt && \
               exit 1; \
             fi && \
             exit 0",
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
        ExceptionMapping::new(CallbackCommand::RuntimeError)
            .add(
                "coverage_below_threshold",
                ExceptionEntry {
                    command: CallbackCommand::AutoFix,
                    max_retries: 9,
                    context_paths: vec![
                        "pipelight-misc/jacoco-report/jacoco.xml".into(),
                        "pipelight-misc/jacoco-report/uncovered.txt".into(),
                        "pipelight-misc/jacoco-report/jacoco-summary.txt".into(),
                        "pipelight-misc/jacoco-report/threshold-fail.txt".into(),
                        "pipelight-misc/jacoco-config.yml".into(),
                        "pipelight-misc/git-diff-report/staged.txt".into(),
                        "pipelight-misc/git-diff-report/unstaged.txt".into(),
                        "pipelight-misc/git-diff-report/untracked.txt".into(),
                        "pipelight-misc/git-diff-report/unpushed.txt".into(),
                    ],
                },
            )
            .add(
                "config_not_found",
                ExceptionEntry {
                    command: CallbackCommand::AutoGenJacocoConfig,
                    max_retries: 9,
                    context_paths: self.source_paths.clone(),
                },
            )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        if stderr.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            Some("config_not_found".into())
        } else if stdout.contains("JaCoCo Total:") {
            Some("coverage_below_threshold".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("PIPELIGHT_CALLBACK:auto_gen_jacoco_config") {
            return "jacoco: config not found (callback)".into();
        }
        if output.contains("no changed java/kt files") {
            return "jacoco: skipped (no changed files)".into();
        }
        if output.contains("all changed files excluded") {
            return "jacoco: skipped (all excluded)".into();
        }
        if output.contains("no exec file") {
            return "jacoco: skipped (no exec file)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
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
            project_type: ProjectType::Gradle,
            language_version: Some("17".into()),
            framework: None,
            image: "gradle:8-jdk17".into(),
            build_cmd: vec!["./gradlew assemble".into()],
            test_cmd: vec!["./gradlew test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/main/java/".into()],
            config_files: vec!["build.gradle".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_basic_step_config() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco");
        assert_eq!(cfg.depends_on, vec!["test".to_string()]);
        assert_eq!(cfg.tag, "non-full");
        assert!(!cfg.allow_failure);
        assert!(cfg.active);
    }

    #[test]
    fn test_command_emits_auto_gen_config_callback_when_missing() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
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
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("pipelight-misc/git-diff-report/unstaged.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/staged.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/untracked.txt"));
        assert!(cmd.contains("pipelight-misc/git-diff-report/unpushed.txt"));
        assert!(cmd.contains("java|kt"));
    }

    #[test]
    fn test_command_skips_when_no_changed_java_files() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("no changed java/kt files"),
            "command must self-skip when no changes, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_applies_exclude_patterns_from_config() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("jacoco-config.yml") && cmd.contains("exclude"),
            "command must consult exclude patterns, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_skips_when_all_files_excluded() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("all changed files excluded"),
            "command must self-skip when filter empties the list, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_handles_missing_exec_file() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("no exec file"),
            "command must handle missing exec gracefully, got: {}",
            cmd
        );
    }

    #[test]
    fn test_command_downloads_jacococli_in_standalone_mode() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
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
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("report") && cmd.contains("--xml"),
            "standalone mode must run `jacococli report --xml`, got: {}",
            cmd
        );
        assert!(cmd.contains("pipelight-misc/jacoco-report/jacoco.xml"));
    }

    #[test]
    fn test_command_parses_xml_for_line_coverage() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("sourcefile") && cmd.contains("LINE"),
            "command must scan sourcefile + LINE counters (first 300 chars): {}",
            &cmd.chars().take(300).collect::<String>()
        );
    }

    #[test]
    fn test_command_reads_threshold_from_config() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("threshold"),
            "command must read threshold from config, got first 500 chars: {}",
            &cmd.chars().take(500).collect::<String>()
        );
    }

    #[test]
    fn test_command_writes_three_report_files() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("jacoco-summary.txt"));
        assert!(cmd.contains("uncovered.txt"));
        assert!(cmd.contains("threshold-fail.txt"));
    }

    #[test]
    fn test_command_emits_total_marker_and_exits_1_on_failure() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("JaCoCo Total:"),
            "command must emit 'JaCoCo Total:' marker for match_exception"
        );
        assert!(cmd.contains("exit 1"));
    }

    #[test]
    fn test_coverage_below_triggers_auto_fix() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "JaCoCo Total: 3 files below 70%",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 9);
        assert_eq!(resolved.exception_key, "coverage_below_threshold");
    }

    #[test]
    fn test_config_not_found_triggers_auto_gen() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "",
            "PIPELIGHT_CALLBACK:auto_gen_jacoco_config",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoGenJacocoConfig);
        assert_eq!(resolved.max_retries, 9);
    }

    #[test]
    fn test_to_on_failure_has_expected_exceptions() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let of = step.exception_mapping().to_on_failure();
        assert_eq!(of.callback_command, CallbackCommand::RuntimeError);
        assert!(of.exceptions.contains_key("coverage_below_threshold"));
        assert!(of.exceptions.contains_key("config_not_found"));
        let cov = &of.exceptions["coverage_below_threshold"];
        assert_eq!(cov.command, CallbackCommand::AutoFix);
        assert_eq!(cov.max_retries, 9);
        assert!(cov
            .context_paths
            .iter()
            .any(|p| p.contains("jacoco-report/jacoco.xml")));
        assert!(cov
            .context_paths
            .iter()
            .any(|p| p.contains("uncovered.txt")));
    }

    #[test]
    fn test_report_str_skip_no_changed_files() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            true,
            "jacoco: no changed java/kt files on current branch — skipping",
            "",
        );
        assert_eq!(r, "jacoco: skipped (no changed files)");
    }

    #[test]
    fn test_report_str_skip_all_excluded() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            true,
            "jacoco: all changed files excluded by jacoco-config.yml — skipping",
            "",
        );
        assert_eq!(r, "jacoco: skipped (all excluded)");
    }

    #[test]
    fn test_report_str_config_not_found() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(false, "", "PIPELIGHT_CALLBACK:auto_gen_jacoco_config ...");
        assert_eq!(r, "jacoco: config not found (callback)");
    }

    #[test]
    fn test_report_str_total_line() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            false,
            "some prefix\nJaCoCo Total: 2 files below 70%\nextra",
            "",
        );
        assert_eq!(r, "JaCoCo Total: 2 files below 70%");
    }

    #[test]
    fn test_report_str_success_default() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(true, "JaCoCo Total: 0 files below 70%", "");
        assert_eq!(r, "JaCoCo Total: 0 files below 70%");
    }

    #[test]
    fn test_report_str_skip_no_exec() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let r = step.output_report_str(
            true,
            "jacoco: no exec file at pipelight-misc/jacoco-report/jacoco.exec ... skipping",
            "",
        );
        assert_eq!(r, "jacoco: skipped (no exec file)");
    }

    #[test]
    fn test_command_uses_gradle_build_dirs() {
        let step = GradleJacocoStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(
            cmd.contains("build/classes/java/main"),
            "Gradle step must look in Gradle's compiled output dir (first 500 chars): {}",
            &cmd.chars().take(500).collect::<String>()
        );
    }
}
