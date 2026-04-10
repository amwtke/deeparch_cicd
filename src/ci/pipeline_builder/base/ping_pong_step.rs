use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::pipeline_builder::{StepConfig, StepDef};

const TOTAL_ROUNDS: u32 = 10;

pub struct PingPongStep;

impl PingPongStep {
    pub fn new() -> Self {
        Self
    }
}

impl StepDef for PingPongStep {
    fn config(&self) -> StepConfig {
        // Use a counter file to track rounds.
        // Rounds 1-9: print "ping" and exit 1 (triggers retry via Ping callback).
        // Round 10: print "ping" and exit 0 (step succeeds, pipeline continues).
        let script = format!(
            concat!(
                "COUNTER_FILE=/tmp/pipelight-ping-pong-$$PPID; ",
                "COUNT=$(cat \"$COUNTER_FILE\" 2>/dev/null || echo 0); ",
                "COUNT=$((COUNT + 1)); ",
                "echo \"$COUNT\" > \"$COUNTER_FILE\"; ",
                "echo \"ping (round $COUNT/{total})\"; ",
                "if [ \"$COUNT\" -ge {total} ]; then rm -f \"$COUNTER_FILE\"; exit 0; fi; ",
                "exit 1",
            ),
            total = TOTAL_ROUNDS,
        );

        StepConfig {
            name: "ping-pong".into(),
            local: true,
            commands: vec![script],
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Ping).add(
            "ping",
            ExceptionEntry {
                command: CallbackCommand::Ping,
                max_retries: TOTAL_ROUNDS - 1,
                context_paths: vec![],
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        // All failures are ping rounds
        Some("ping".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, _stderr: &str) -> String {
        if success {
            format!("Ping-pong completed ({} rounds)", TOTAL_ROUNDS)
        } else {
            // Extract round info from stdout
            let line = stdout.lines().find(|l| l.contains("ping (round"));
            match line {
                Some(l) => l.trim().to_string(),
                None => "ping".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::action::CallbackCommandAction;
    use crate::ci::callback::command::CallbackCommandRegistry;

    #[test]
    fn test_config() {
        let step = PingPongStep::new();
        let cfg = step.config();
        assert_eq!(cfg.name, "ping-pong");
        assert!(cfg.local);
        assert!(cfg.image.is_empty());
        assert!(cfg.depends_on.is_empty());
        assert_eq!(cfg.commands.len(), 1);
        assert!(cfg.commands[0].contains("ping"));
    }

    #[test]
    fn test_exception_mapping_resolves_to_ping() {
        let step = PingPongStep::new();
        let match_fn = |exit_code: i64, stdout: &str, stderr: &str| -> Option<String> {
            step.match_exception(exit_code, stdout, stderr)
        };
        let resolved =
            step.exception_mapping()
                .resolve(1, "ping (round 1/10)\n", "", Some(&match_fn));
        assert_eq!(resolved.command, CallbackCommand::Ping);
        assert_eq!(resolved.max_retries, TOTAL_ROUNDS - 1);
        assert_eq!(resolved.exception_key, "ping");
    }

    #[test]
    fn test_ping_action_is_retry() {
        let registry = CallbackCommandRegistry::new();
        assert_eq!(
            registry.action_for(&CallbackCommand::Ping),
            CallbackCommandAction::Retry
        );
    }

    #[test]
    fn test_report_success() {
        let step = PingPongStep::new();
        let report = step.output_report_str(true, "ping (round 10/10)\n", "");
        assert_eq!(
            report,
            format!("Ping-pong completed ({} rounds)", TOTAL_ROUNDS)
        );
    }

    #[test]
    fn test_report_in_progress() {
        let step = PingPongStep::new();
        let report = step.output_report_str(false, "ping (round 3/10)\n", "");
        assert_eq!(report, "ping (round 3/10)");
    }

    #[test]
    fn test_report_no_output() {
        let step = PingPongStep::new();
        let report = step.output_report_str(false, "", "");
        assert_eq!(report, "ping");
    }

    #[test]
    fn test_exception_mapping_to_on_failure() {
        let step = PingPongStep::new();
        let of = step.exception_mapping().to_on_failure();
        assert_eq!(of.callback_command, CallbackCommand::Ping);
        assert_eq!(of.max_retries, TOTAL_ROUNDS - 1);
        assert!(of.context_paths.is_empty());
    }
}
