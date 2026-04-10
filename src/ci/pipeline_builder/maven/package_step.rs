use crate::ci::callback::command::CallbackCommand;
use crate::ci::callback::exception::ExceptionMapping;
use crate::ci::detector::ProjectInfo;
use crate::ci::pipeline_builder::{StepConfig, StepDef};

pub struct PackageStep {
    image: String,
    subdir: Option<String>,
}

impl PackageStep {
    pub fn new(info: &ProjectInfo) -> Self {
        Self {
            image: info.image.clone(),
            subdir: info.subdir.clone(),
        }
    }
}

impl StepDef for PackageStep {
    fn config(&self) -> StepConfig {
        let cmd = match &self.subdir {
            Some(subdir) => format!("cd {} && mvn package -DskipTests", subdir),
            None => "mvn package -DskipTests".into(),
        };
        StepConfig {
            name: "package".into(),
            image: self.image.clone(),
            commands: vec![cmd],
            depends_on: vec!["test".into()],
            on_failure: None,
            ..Default::default()
        }
    }

    fn exception_mapping(&self) -> ExceptionMapping {
        ExceptionMapping::new(CallbackCommand::Abort)
    }

    fn output_report_str(&self, success: bool, _stdout: &str, _stderr: &str) -> String {
        if success {
            "Package created".into()
        } else {
            "Package failed".into()
        }
    }
}
