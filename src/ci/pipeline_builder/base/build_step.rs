use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::{ExceptionEntry, ExceptionMapping};
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{count_pattern, StepConfig, StepDef};

pub struct BuildStep {
    pub image: String,
    pub build_cmd: Vec<String>,
    #[allow(dead_code)]
    pub source_paths: Vec<String>,
    #[allow(dead_code)]
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
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::AutoFix).add(
            "compile_error",
            ExceptionEntry {
                command: CallbackCommand::AutoFix,
                max_retries: 9,
                context_paths: [&self.source_paths[..], &self.config_files[..]].concat(),
            },
        )
    }

    fn match_exception(&self, _exit_code: i64, _stdout: &str, _stderr: &str) -> Option<String> {
        Some("compile_error".into())
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
    }

    #[test]
    fn test_exception_mapping() {
        use crate::ci::callback::command::CallbackCommand;
        let step = BuildStep::new(&make_info());
        let mapping = step.exception_mapping();
        let resolved = mapping.resolve(
            1,
            "",
            "some compile error",
            Some(&|ec, out, err| step.match_exception(ec, out, err)),
        );
        assert_eq!(resolved.command, CallbackCommand::AutoFix);
        assert_eq!(resolved.max_retries, 9);
        assert!(resolved.context_paths.contains(&"src/".to_string()));
        assert!(resolved.context_paths.contains(&"Cargo.toml".to_string()));
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
