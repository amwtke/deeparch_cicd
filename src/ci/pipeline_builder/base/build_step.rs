use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{CallbackCommand, OnFailure};
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct BuildStep {
    pub image: String,
    pub build_cmd: Vec<String>,
    pub source_paths: Vec<String>,
    pub config_files: Vec<String>,
}

impl BuildStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            build_cmd: info.build_cmd.clone(),
            source_paths: info.source_paths.clone(),
            config_files: info.config_files.clone(),
        }
    }
}

impl StepDef for BuildStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "build".into(),
            image: self.image.clone(),
            commands: self.build_cmd.clone(),
            on_failure: Some(OnFailure {
                callback_command: CallbackCommand::AutoFix,
                max_retries: 3,
                context_paths: [&self.source_paths[..], &self.config_files[..]].concat(),
            }),
            ..Default::default()
        }
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        let warning_count = count_pattern(&output, &["warning:", "WARNING", "[WARNING]"]);
        if success {
            if warning_count > 0 {
                format!("Build succeeded ({} warnings)", warning_count)
            } else {
                "Build succeeded".into()
            }
        } else {
            let error_count = count_pattern(&output, &["error:", "ERROR", "[ERROR]"]);
            if error_count > 0 {
                format!("Build failed ({} errors)", error_count)
            } else {
                "Build failed".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};

    fn make_info() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: None,
            framework: None,
            image: "rust:latest".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_config() {
        let step = BuildStep::new(&make_info());
        let cfg = step.config();
        assert_eq!(cfg.name, "build");
        assert_eq!(cfg.image, "rust:latest");
        assert_eq!(cfg.commands, vec!["cargo build"]);
        assert!(cfg.depends_on.is_empty());
        let on_failure = cfg.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.callback_command, CallbackCommand::AutoFix);
        assert_eq!(on_failure.max_retries, 3);
        assert!(on_failure.context_paths.contains(&"src/".to_string()));
        assert!(on_failure.context_paths.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_report_success() {
        let step = BuildStep::new(&make_info());
        let report = step.output_report_str(true, "Compiling foo v0.1.0\n", "");
        assert_eq!(report, "Build succeeded");
    }

    #[test]
    fn test_report_warnings() {
        let step = BuildStep::new(&make_info());
        let report =
            step.output_report_str(true, "warning: unused variable\nwarning: dead code\n", "");
        assert_eq!(report, "Build succeeded (2 warnings)");
    }

    #[test]
    fn test_report_failure() {
        let step = BuildStep::new(&make_info());
        let report =
            step.output_report_str(false, "", "error: cannot find value\nerror: aborting\n");
        assert_eq!(report, "Build failed (2 errors)");
    }
}
