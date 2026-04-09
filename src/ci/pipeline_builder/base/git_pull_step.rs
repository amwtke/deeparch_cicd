use crate::ci::parser::{CallbackCommand, OnFailure};
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct GitPullStep;

impl GitPullStep {
    pub fn new() -> Self {
        Self
    }
}

impl StepDef for GitPullStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "git-pull".into(),
            local: true,
            commands: vec![
                "if [ ! -d .git ]; then echo 'Not a git repository, skipping'; exit 0; fi".into(),
                "if ! git remote | grep -q .; then echo 'No remote configured, skipping'; exit 0; fi".into(),
                "echo \"Pulling from $(git remote get-url origin 2>/dev/null || git remote get-url $(git remote | head -1))...\"".into(),
                "STASHED=false; if ! git diff --quiet || ! git diff --cached --quiet; then echo 'Stashing local changes...'; git stash && STASHED=true; fi".into(),
                "git pull --rebase || { if $STASHED; then git stash pop; fi; echo 'ERROR: git pull --rebase failed — possible merge conflict'; exit 1; }".into(),
                "if $STASHED; then echo 'Restoring stashed changes...'; git stash pop || { echo 'ERROR: stash pop conflict — run git stash pop manually'; exit 1; }; fi".into(),
            ],
            on_failure: Some(OnFailure {
                callback_command: CallbackCommand::Abort,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, _success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if output.contains("Already up to date") || output.contains("Already up-to-date") {
            "Already up to date".into()
        } else if output.contains("skipping") || output.contains("Skipping") {
            let line = output
                .lines()
                .find(|l| l.contains("skipping"))
                .unwrap_or("Skipped");
            line.trim().into()
        } else if output.contains("files changed") || output.contains("file changed") {
            output
                .lines()
                .find(|l| l.contains("files changed") || l.contains("file changed"))
                .unwrap_or("Pulled latest changes")
                .trim()
                .into()
        } else if output.contains("Pulling") {
            "Pulled latest changes".into()
        } else {
            "OK".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::parser::CallbackCommand;

    #[test]
    fn test_config() {
        let step = GitPullStep::new();
        let cfg = step.config();
        assert_eq!(cfg.name, "git-pull");
        assert!(cfg.local);
        assert!(cfg.image.is_empty());
        assert!(cfg.depends_on.is_empty());
        assert!(cfg.volumes.is_empty());
        let on_failure = cfg.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.callback_command, CallbackCommand::Abort);
        assert_eq!(on_failure.max_retries, 0);
    }

    #[test]
    fn test_report_up_to_date() {
        let step = GitPullStep::new();
        let report = step.output_report_str(true, "Already up to date.\n", "");
        assert_eq!(report, "Already up to date");
    }

    #[test]
    fn test_report_skipped() {
        let step = GitPullStep::new();
        let report = step.output_report_str(true, "Not a git repository, skipping\n", "");
        assert_eq!(report, "Not a git repository, skipping");
    }
}
