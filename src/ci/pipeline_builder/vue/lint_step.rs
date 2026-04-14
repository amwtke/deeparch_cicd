use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct VueLintStep {
    image: String,
    commands: Vec<String>,
    depends_on: Vec<String>,
    context_paths: Vec<String>,
    hygiene_sensitive: bool,
    hygiene_context: Vec<String>,
}

impl VueLintStep {
    pub fn new(info: &ProjectInfo, depends_on: Vec<String>) -> Option<Self> {
        Self::new_impl(info, depends_on, true)
    }

    /// `ProjectInfo.lint_cmd` was filled from saved pipeline YAML (commands already final).
    pub(crate) fn new_from_stored_pipeline(
        info: &ProjectInfo,
        depends_on: Vec<String>,
    ) -> Option<Self> {
        Self::new_impl(info, depends_on, false)
    }

    fn new_impl(
        info: &ProjectInfo,
        depends_on: Vec<String>,
        prepend_hygiene_when_root: bool,
    ) -> Option<Self> {
        let raw = info.lint_cmd.clone()?;
        let hygiene_sensitive = depends_on.is_empty();
        let commands = if hygiene_sensitive && prepend_hygiene_when_root {
            super::hygiene_step::prepend_hygiene(raw)
        } else {
            raw
        };
        let hygiene_context = if hygiene_sensitive {
            super::hygiene_step::hygiene_context_paths(info)
        } else {
            vec![]
        };
        Some(Self {
            image: info.image.clone(),
            commands,
            depends_on,
            context_paths: super::vue_lint_context_paths(info),
            hygiene_sensitive,
            hygiene_context,
        })
    }
}

impl StepDef for VueLintStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "lint".into(),
            image: self.image.clone(),
            commands: self.commands.clone(),
            depends_on: self.depends_on.clone(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        let mut m = ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "lint_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 9,
                context_paths: self.context_paths.clone(),
            },
        );
        if self.hygiene_sensitive {
            m = m.add(
                "hygiene_error",
                ExceptionEntry {
                    command: CallbackCommand::RuntimeError,
                    max_retries: 0,
                    context_paths: self.hygiene_context.clone(),
                },
            );
        }
        m
    }

    fn match_exception(&self, _exit_code: i64, stdout: &str, stderr: &str) -> Option<String> {
        if self.hygiene_sensitive && super::hygiene_step::hygiene_failure_in_output(stdout, stderr)
        {
            return Some("hygiene_error".into());
        }
        Some("lint_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let warning_count = count_pattern(&output, &["warning:", "WARNING", "[WARN]"]);
        let violation_count = count_pattern(&output, &["violation", "Violation"]);
        let issues = warning_count + violation_count;
        if success {
            if issues > 0 {
                format!("lint: passed ({} warnings)", issues)
            } else {
                "lint: no issues found".into()
            }
        } else if issues > 0 {
            format!("lint: {} issues found", issues)
        } else {
            "lint: failed".into()
        }
    }
}
