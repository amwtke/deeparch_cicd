use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::base::{JacocoMode, JACOCO_VERSION};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// Full-repo JaCoCo scan (tag = "full").
///
/// Activated by `--full-report-only`. Scans every `src/main/{java,kotlin}`
/// dir with the same `pipelight-misc/jacoco-config.yml` exclude rules but
/// without git-diff filtering. Never auto-fixes — findings are surfaced via
/// `jacoco_print_command` so the LLM prints a grouped-by-package table;
/// pipeline is not blocked (`allow_failure: true`).
pub struct MavenJacocoFullStep {
    image: String,
    source_paths: Vec<String>,
    subdir: Option<String>,
    mode: JacocoMode,
}

impl MavenJacocoFullStep {
    pub fn new(info: &ProjectInfo, mode: JacocoMode) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
            mode,
        }
    }
}

impl StepDef for MavenJacocoFullStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}if [ ! -f /workspace/pipelight-misc/jacoco-config.yml ]; then \
               echo 'PIPELIGHT_CALLBACK:auto_gen_jacoco_config - No jacoco-config.yml found.' >&2; exit 1; \
             fi && \
             if [ ! -f /workspace/pipelight-misc/jacoco-report/jacoco.exec ]; then \
               echo 'jacoco_full: no exec file — skipping'; exit 0; \
             fi && \
             JACOCO_VER={ver} && \
             JACOCO_CACHE=$HOME/.pipelight/cache && \
             JACOCO_DIR=$JACOCO_CACHE/jacoco-$JACOCO_VER && \
             if [ ! -f $JACOCO_DIR/lib/jacococli.jar ]; then \
               mkdir -p $JACOCO_DIR && \
               curl -sL https://repo1.maven.org/maven2/org/jacoco/jacoco/$JACOCO_VER/jacoco-$JACOCO_VER.zip -o /tmp/jacoco.zip && \
               (cd $JACOCO_DIR && jar xf /tmp/jacoco.zip || unzip -o /tmp/jacoco.zip) && rm -f /tmp/jacoco.zip; \
             fi && \
             mkdir -p /workspace/pipelight-misc/jacoco-full-report && \
             CLASS_DIRS=$(find . -path '*/target/classes' -type d 2>/dev/null | tr '\\n' ' ') && \
             SRC_DIRS=$(find . \\( -path '*/src/main/java' -o -path '*/src/main/kotlin' \\) -type d 2>/dev/null | tr '\\n' ' ') && \
             CLASSFILES_ARGS=$(for d in $CLASS_DIRS; do echo -n \"--classfiles $d \"; done) && \
             SOURCES_ARGS=$(for d in $SRC_DIRS; do echo -n \"--sourcefiles $d \"; done) && \
             java -jar $JACOCO_DIR/lib/jacococli.jar report \
               /workspace/pipelight-misc/jacoco-report/jacoco.exec \
               $CLASSFILES_ARGS $SOURCES_ARGS \
               --xml /workspace/pipelight-misc/jacoco-full-report/jacoco.xml \
               --html /workspace/pipelight-misc/jacoco-full-report/html; \
             THRESHOLD=$(awk '/^threshold:/{{print $2; exit}}' /workspace/pipelight-misc/jacoco-config.yml) && \
             THRESHOLD=${{THRESHOLD:-70}} && \
             REPORT=/workspace/pipelight-misc/jacoco-full-report && \
             : > $REPORT/jacoco-summary.txt && \
             : > $REPORT/threshold-fail.txt && \
             awk -v threshold=\"$THRESHOLD\" \
                 -v summary=\"$REPORT/jacoco-summary.txt\" \
                 -v failed=\"$REPORT/threshold-fail.txt\" '\
               /<package/ {{ match($0, /name=\"[^\"]*\"/); pkg=substr($0, RSTART+6, RLENGTH-7) }} \
               /<sourcefile/ {{ match($0, /name=\"[^\"]*\"/); sf=substr($0, RSTART+6, RLENGTH-7); have=0 }} \
               /<counter type=\"LINE\"/ && sf!=\"\" && have==0 {{ \
                 have=1; match($0,/missed=\"[0-9]+\"/); mi=substr($0,RSTART+8,RLENGTH-9)+0; \
                 match($0,/covered=\"[0-9]+\"/); co=substr($0,RSTART+9,RLENGTH-10)+0; \
                 t=mi+co; pct=(t==0)?100:int((co*1000)/t)/10; \
                 rel=(pkg==\"\")?sf:pkg\"/\"sf; \
                 printf(\"%s %.1f%%\\n\", rel, pct) >> summary; \
                 if (pct<threshold) printf(\"%s %.1f%%\\n\", rel, pct) >> failed; \
                 sf=\"\"; \
               }} \
             ' $REPORT/jacoco.xml && \
             FAIL_COUNT=$(wc -l < $REPORT/threshold-fail.txt | tr -d ' ') && \
             echo \"\" && echo \"JaCoCo Total: $FAIL_COUNT files below $THRESHOLD%\" && \
             echo \"jacoco_full: report at /workspace/pipelight-misc/jacoco-full-report/\" && \
             exit 0",
            cd = cd_prefix,
            ver = JACOCO_VERSION,
        );
        StepConfig {
            name: "jacoco_full".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["jacoco".into()],
            allow_failure: true,
            active: false,
            tag: "full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError).add(
            "coverage_below_threshold",
            ExceptionEntry {
                command: CallbackCommand::JacocoPrintCommand,
                max_retries: 0,
                context_paths: vec![
                    "pipelight-misc/jacoco-full-report/jacoco.xml".into(),
                    "pipelight-misc/jacoco-full-report/jacoco-summary.txt".into(),
                    "pipelight-misc/jacoco-full-report/threshold-fail.txt".into(),
                ],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, _stderr: &str) -> Option<String> {
        if stdout.contains("JaCoCo Total:") {
            Some("coverage_below_threshold".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("no exec file") {
            return "jacoco_full: skipped (no exec file)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("JaCoCo Total:")) {
            return line.trim().to_string();
        }
        if success {
            "jacoco_full: ok".into()
        } else {
            "jacoco_full: failed".into()
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
    fn test_full_step_config() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cfg = step.config();
        assert_eq!(cfg.name, "jacoco_full");
        assert_eq!(cfg.depends_on, vec!["jacoco".to_string()]);
        assert_eq!(cfg.tag, "full");
        assert!(cfg.allow_failure);
        assert!(!cfg.active);
    }

    #[test]
    fn test_full_step_uses_print_command() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let resolved = step.exception_mapping().resolve(
            1,
            "JaCoCo Total: 5 files below 70%",
            "",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::JacocoPrintCommand);
        assert_eq!(resolved.exception_key, "coverage_below_threshold");
    }

    #[test]
    fn test_full_step_reports_to_full_report_dir() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("pipelight-misc/jacoco-full-report"));
    }

    #[test]
    fn test_full_step_total_marker() {
        let step = MavenJacocoFullStep::new(&make_info(), JacocoMode::Standalone);
        let cmd = step.config().commands[0].clone();
        assert!(cmd.contains("JaCoCo Total:"));
    }
}
