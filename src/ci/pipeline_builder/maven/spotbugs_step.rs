use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{git_changed_files_snippet, StepConfig, StepDef};

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
        // SpotBugs with two modes (kept in sync with pmd_step.rs):
        //
        //   Incremental mode (git repo present, PIPELIGHT_FULL_REPORT_ONLY unset):
        //     Scans only bytecode for java sources changed on the current branch (unstaged,
        //     staged, and local commits ahead of @{upstream}). Maps each changed .java to
        //     its .class files under */target/classes and feeds them as analysis targets.
        //     If no changes → skip. Bugs → spotbugs_print (report-only; allow_failure=true).
        //
        //   Full-scan mode (PIPELIGHT_FULL_REPORT_ONLY=1, OR no git repo):
        //     Scans every */target/classes directory and exits 0 (report-only).
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
             CLASS_DIRS=$(find . -path '*/target/classes' -type d | tr '\\n' ' ') && \
             if [ -z \"$CLASS_DIRS\" ]; then \
               echo 'No compiled classes found (target/classes). Run build step first.' >&2; exit 1; \
             fi && \
             FULL_SCAN=${{PIPELIGHT_FULL_REPORT_ONLY:-0}} && \
             if [ \"$FULL_SCAN\" = \"1\" ]; then \
               echo 'SpotBugs: --full-report-only requested — full scan (report-only)'; \
             elif ! git rev-parse --git-dir >/dev/null 2>&1; then \
               echo 'SpotBugs: not a git repository — full scan (report-only)'; \
               FULL_SCAN=1; \
             fi && \
             if [ \"$FULL_SCAN\" = \"0\" ]; then \
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
               SB_TARGETS=\"$ANALYZE_TARGETS\"; \
             else \
               SB_TARGETS=\"$CLASS_DIRS\"; \
             fi && \
             AUX_CP=\"\" && \
             for dep_dir in $(find . -path '*/target/dependency' -type d 2>/dev/null); do \
               for jar in $dep_dir/*.jar; do \
                 [ -f \"$jar\" ] && AUX_CP=\"$AUX_CP:$jar\"; \
               done; \
             done && \
             M2_JARS=$(find $HOME/.m2/repository -name '*.jar' 2>/dev/null | head -500 | tr '\\n' ':') && \
             AUX_CP=\"$AUX_CP:$M2_JARS\" && \
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
               $SB_TARGETS \
               2>/tmp/sb-stderr.log; \
             SB_EXIT=$?; \
             BUGS=$(grep -c '<BugInstance' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null || echo 0); \
             echo \"\"; echo \"SpotBugs Total: $BUGS bugs found\"; \
             if [ \"$BUGS\" -gt 0 ]; then \
               echo \"\"; echo \"=== Bugs by Category ===\"; \
               grep -o 'category=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/category=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"=== Bugs by Priority ===\"; \
               grep -o 'priority=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/priority=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"=== Top 10 Bug Types ===\"; \
               grep -o 'type=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/type=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             fi && \
             $SB_DIR/bin/spotbugs -textui \
               -html \
               -output /workspace/pipelight-misc/spotbugs-report/spotbugs-result.html \
               -auxclasspath \"$AUX_CP\" \
               -low \
               $EXCLUDE_OPT \
               $SB_TARGETS 2>/dev/null || true && \
             ( echo \"SpotBugs Report Summary\"; echo \"======================\"; \
               echo \"Total bugs: $BUGS\"; echo \"\"; \
               echo \"By Category:\"; \
               grep -o 'category=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/category=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"By Priority:\"; \
               grep -o 'priority=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/priority=\"//;s/\"//' | sort | uniq -c | sort -rn; \
               echo \"\"; echo \"Top 10 Bug Types:\"; \
               grep -o 'type=\"[^\"]*\"' /workspace/pipelight-misc/spotbugs-report/spotbugs-result.xml 2>/dev/null \
                 | sed 's/type=\"//;s/\"//' | sort | uniq -c | sort -rn | head -10; \
             ) > /workspace/pipelight-misc/spotbugs-report/spotbugs-summary.txt 2>/dev/null; \
             if [ \"$FULL_SCAN\" = \"1\" ]; then \
               echo \"SpotBugs: full-scan report-only mode — report at /workspace/pipelight-misc/spotbugs-report/\"; \
               exit 0; \
             fi; \
             if [ \"$BUGS\" -gt 0 ]; then exit 1; fi; \
             exit 0",
            cd = cd_prefix,
            changed_files = git_changed_files_snippet(&["*.java"])
        );
        StepConfig {
            name: "spotbugs".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["build".into()],
            // Report-only: bugs surface via spotbugs_print_command.
            allow_failure: true,
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
        if output.contains("no changed java files")
            || output.contains("changed java files have no matching compiled classes")
        {
            return "spotbugs: skipped (no changed files)".into();
        }
        // Extract "SpotBugs Total: N bugs found" line from shell output
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
