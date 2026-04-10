use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::ExceptionMapping;
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{StepConfig, StepDef};

pub struct TestStep {
    pub image: String,
    pub test_cmd: Vec<String>,
    pub test_parser: Option<fn(&str) -> Option<String>>,
}

impl TestStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            test_cmd: info.test_cmd.clone(),
            test_parser: None,
        }
    }

    pub fn with_parser(mut self, parser: fn(&str) -> Option<String>) -> Self {
        self.test_parser = Some(parser);
        self
    }
}

impl StepDef for TestStep {
    fn config(&self) -> StepConfig {
        StepConfig {
            name: "test".into(),
            image: self.image.clone(),
            commands: self.test_cmd.clone(),
            depends_on: vec!["build".into()],
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }

    fn output_report_str(&self, success: bool, stdout: &str, stderr: &str) -> String {
        let output = format!("{}{}", stdout, stderr);
        if let Some(parser) = self.test_parser {
            if let Some(report) = parser(&output) {
                return report;
            }
        }
        if success {
            "Tests passed".into()
        } else {
            "Tests failed".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::callback::command::CallbackCommand;
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
        let step = TestStep::new(&make_info());
        let cfg = step.config();
        assert_eq!(cfg.name, "test");
        assert_eq!(cfg.depends_on, vec!["build"]);
    }

    #[test]
    fn test_exception_mapping() {
        let step = TestStep::new(&make_info());
        let resolved = step
            .exception_mapping()
            .resolve(1, "", "some test failure", None);
        assert_eq!(resolved.command, CallbackCommand::Abort);
        assert_eq!(resolved.max_retries, 0);
    }

    #[test]
    fn test_generic_report() {
        let step = TestStep::new(&make_info());
        assert_eq!(step.output_report_str(true, "ok", ""), "Tests passed");
        assert_eq!(step.output_report_str(false, "FAIL", ""), "Tests failed");
    }

    #[test]
    fn test_custom_parser() {
        fn my_parser(output: &str) -> Option<String> {
            if output.contains("42 passed") {
                Some("42 tests passed".into())
            } else {
                None
            }
        }

        let step = TestStep::new(&make_info()).with_parser(my_parser);
        assert_eq!(
            step.output_report_str(true, "42 passed", ""),
            "42 tests passed"
        );
        // Falls back to generic when parser returns None
        assert_eq!(
            step.output_report_str(true, "some other output", ""),
            "Tests passed"
        );
    }
}
