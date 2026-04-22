use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{git_changed_files_snippet, StepConfig, StepDef};

/// Incremental SpotBugs step (tag = "non-full").
pub struct SpotbugsStep {
    image: String,
    #[allow(dead_code)]
    source_paths: Vec<String>,
    subdir: Option<String>,
}

impl SpotbugsStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            source_paths: info.source_paths.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for SpotbugsStep {
    fn config(&self) -> StepConfig {
        let cd_prefix = match &self.subdir {
            Some(subdir) => format!("cd {} && ", subdir),
            None => String::new(),
        };
        let cmd = format!(
            "{cd}echo 'Recompiling before SpotBugs scan...' && \
             {compile_cmd} && \
             SB_VER=4.8.6 && \
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
             {changed_files} && \
             if [ -z \"$CHANGED_FILES\" ]; then \
               echo 'SpotBugs: no changed java files on current branch — skipping'; \
               exit 0; \
             fi && \
             printf '%s\\n' \"$CHANGED_FILES\" > /tmp/sb-changed && \
             : > /tmp/sb-targets && \
             while IFS= read -r jf; do \
               rel=$(echo \"$jf\" | sed -n 's|.*/src/main/java/||p'); \
               [ -z \"$rel\" ] && continue; \
               base=${{rel%.java}}; \
               for cdir in $CLASS_DIRS; do \
                 cdir_stripped=$(echo \"$cdir\" | sed 's:/*$::'); \
                 pkg_dir=\"$cdir_stripped/$(dirname \"$base\")\"; \
                 class_base=$(basename \"$base\"); \
                 if [ -d \"$pkg_dir\" ]; then \
                   for cf in \"$pkg_dir/$class_base.class\" \"$pkg_dir/$class_base\"\\$*.class; do \
                     [ -f \"$cf\" ] && printf '%s ' \"$cf\" >> /tmp/sb-targets; \
                   done; \
                 fi; \
               done; \
             done < /tmp/sb-changed; \
             ANALYZE_TARGETS=$(cat /tmp/sb-targets); \
             if [ -z \"$ANALYZE_TARGETS\" ]; then \
               echo 'SpotBugs: changed java files have no matching compiled classes — skipping'; \
               exit 0; \
             fi; \
             NUM_FILES=$(echo \"$CHANGED_FILES\" | wc -l | tr -d ' '); \
             NUM_CLASSES=$(echo \"$ANALYZE_TARGETS\" | wc -w | tr -d ' '); \
             echo \"SpotBugs: scanning $NUM_CLASSES compiled class(es) from $NUM_FILES changed java file(s) on current branch\"; \
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
               $ANALYZE_TARGETS \
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
             if [ \"$BUGS\" -gt 0 ]; then exit 1; fi; \
             exit 0",
            cd = cd_prefix,
            compile_cmd = "./gradlew classes -q",
            changed_files = git_changed_files_snippet(&["*.java"], self.subdir.as_deref())
        );
        StepConfig {
            name: "spotbugs".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            allow_failure: false,
            tag: "non-full".into(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::RuntimeError).add(
            "spotbugs_bugs_found",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 9,
                context_paths: vec![
                    "pipelight-misc/spotbugs-report/spotbugs-result.xml".into(),
                    "pipelight-misc/spotbugs-report/spotbugs-summary.txt".into(),
                    "pipelight-misc/git-diff-report/diff.txt".into(),
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
        if output.contains("no changed java files")
            || output.contains("changed java files have no matching compiled classes")
        {
            return "spotbugs: skipped (no changed files)".into();
        }
        if let Some(line) = output.lines().find(|l| l.contains("SpotBugs Total:")) {
            return line.trim().to_string();
        }
        if !success {
            "spotbugs: failed".into()
        } else {
            "spotbugs: no bugs found".into()
        }
    }
}
