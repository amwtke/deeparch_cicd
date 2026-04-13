use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::{ProjectInfo, ProjectType};
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

fn has_tsconfig_signal(info: &ProjectInfo) -> bool {
    info.config_files.iter().any(|f| {
        f == "tsconfig.json"
            || f.ends_with("/tsconfig.json")
            || f == "tsconfig.app.json"
            || f.ends_with("/tsconfig.app.json")
    })
}

fn infer_raw_typecheck_commands(info: &ProjectInfo) -> Option<Vec<String>> {
    if info.project_type != ProjectType::Vue {
        return None;
    }
    if !has_tsconfig_signal(info) {
        return None;
    }
    Some(vec!["npm ci".into(), "npx vue-tsc --noEmit".into()])
}

pub struct VueTypecheckStep {
    image: String,
    commands: Vec<String>,
    depends_on: Vec<String>,
    context_paths: Vec<String>,
    hygiene_sensitive: bool,
    hygiene_context: Vec<String>,
}

impl VueTypecheckStep {
    pub fn new(info: &ProjectInfo, depends_on: Vec<String>) -> Option<Self> {
        let raw = infer_raw_typecheck_commands(info)?;
        Some(Self::from_raw_commands(info, depends_on, raw, true))
    }

    /// Rebuild from a pipeline loaded from YAML (commands are the source of truth).
    pub(crate) fn from_stored_commands(
        info: &ProjectInfo,
        depends_on: Vec<String>,
        commands: Vec<String>,
    ) -> Self {
        Self::from_raw_commands(info, depends_on, commands, false)
    }

    fn from_raw_commands(
        info: &ProjectInfo,
        depends_on: Vec<String>,
        raw: Vec<String>,
        prepend_hygiene_when_root: bool,
    ) -> Self {
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
        Self {
            image: info.image.clone(),
            commands,
            depends_on,
            context_paths: super::vue_typecheck_context_paths(info),
            hygiene_sensitive,
            hygiene_context,
        }
    }
}

impl StepDef for VueTypecheckStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "typecheck".into(),
            image: self.image.clone(),
            commands: self.commands.clone(),
            depends_on: self.depends_on.clone(),
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        let mut m = ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "typecheck_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 2,
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
        Some("typecheck_error".into())
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let errors = count_pattern(&output, &["error TS", "error:", "Error"]);
        if success {
            "typecheck: passed".into()
        } else if errors > 0 {
            format!("typecheck: {} errors", errors)
        } else {
            "typecheck: failed".into()
        }
    }
}
