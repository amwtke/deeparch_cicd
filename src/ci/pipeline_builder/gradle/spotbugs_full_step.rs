use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{StepConfig, StepDef};

/// Full-repo SpotBugs scan (tag = "full"). Activated by `--full-report-only`.
pub struct SpotbugsFullStep {
    image: String,
    #[allow(dead_code)]
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl SpotbugsFullStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for SpotbugsFullStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}SB_VER=4.8.6 && \
             SB_CACHE=$HOME/.pipelight/cache && \
             SB_DIR=$SB_CACHE/spotbugs-$SB_VER && \
             if [ ! -f $SB_DIR/lib/spotbugs.jar ]; then \
               echo 'Downloading SpotBugs CLI...' && \
               mkdir -p $SB_CACHE && \
               curl -sL https://github.com/spotbugs/spotbugs/releases/download/$SB_VER/spotbugs-$SB_VER.tgz \
                 -o /tmp/spotbugs.tgz && \
               tar xzf /tmp/spotbugs.tgz -C $SB_CACHE && \
               chmod +x $SB_DIR/bin/spotbugs && \
               rm -f /tmp/spotbugs.tgz; \
             fi && \
             CLASS_DIRS=$(find . -path '*/build/classes/java/main' -type d | tr '\\n' ' ') && \
             if [ -z \"$CLASS_DIRS\" ]; then \
               echo 'No compiled classes found (build/classes). Run build step first.' >&2; exit 1; \
             fi && \
             echo 'SpotBugs (full): scanning all */build/classes/java/main directories' && \
             AUX_CP=\"\" && \
             GRADLE_CACHE=$HOME/.gradle/caches/modules-2/files-2.1 && \
             if [ -d \"$GRADLE_CACHE\" ]; then \
               AUX_CP=$(find $GRADLE_CACHE -name '*.jar' 2>/dev/null | head -500 | tr '\\n' ':'); \
             fi && \
             for cdir in $CLASS_DIRS; do AUX_CP=\"$AUX_CP:$cdir\"; done && \
             mkdir -p /workspace/pipelight-misc/spotbugs-report && \
             EXCLUDE_OPT=\"\" && \
             if [ -f /workspace/pipelight-misc/spotbugs-exclude.xml ]; then \
               EXCLUDE_OPT=\"-exclude /workspace/pipelight-misc/spotbugs-exclude.xml\"; \
             fi && \
             $SB_DIR/bin/spotbugs -textui \
               -xml:withMessages \
               -output /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml \
               -auxclasspath \"$AUX_CP\" \
               -low \
               $EXCLUDE_OPT \
               $CLASS_DIRS \
               2>/tmp/sb-stderr.log; \
             BUGS=$(grep -c '<BugInstance' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null || echo 0); \
             echo \"\"; echo \"SpotBugs Total: $BUGS bugs found\"; \
             if [ \"$BUGS\" -gt 0 ]; then \
               echo \"\"; echo \"=== Bugs by Category ===\"; \
               grep -o 'category=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/category=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"=== Top 10 Bug Types ===\"; \
               grep -o 'type=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/type=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             fi && \
             ( echo \"SpotBugs Report Summary\"; echo \"======================\"; \
               echo \"Total bugs: $BUGS\"; \
             ) > /workspace/pipelight-misc/spotbugs-report/spotbugs-summary.txt 2>/dev/null; \
             echo \"SpotBugs (full): report at /workspace/pipelight-misc/spotbugs-report/\"; \
             exit 0",
            cd = cd_prefix,
        );
        StepConfig {
            name: "spotbugs_full".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            allow_failure: true,
            active: false,
            tag: "full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError).add(
            "spotbugs_bugs_found",
            ExceptionEntry {
                command: CallbackCommand::SpotbugsPrintCommand,
                max_retries: 0,
                context_paths: vec![
                    "pipelight-misc/spotbugs-report/spotbugs-result.xml".into(),
                    "pipelight-misc/spotbugs-report/spotbugs-summary.txt".into(),
                ],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, _stderr: &str) -> Option<String> {
        if stdout.contains("SpotBugs Total:") {
            Some("spotbugs_bugs_found".into())
        } else {
            None
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if let Some(line) = output.lines().find(|l| l.contains("SpotBugs Total:")) {
            return line.trim().to_string();
        }
        if !success {
            "spotbugs_full: failed".into()
        } else {
            "spotbugs_full: no bugs found".into()
        }
    }
}
