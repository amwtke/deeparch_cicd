use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::builder::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    let cmd = match &info.subdir {
        Some(subdir) => format!("cd {} && mvn checkstyle:check", subdir),
        None => "mvn checkstyle:check".into(),
    };
    StepDef {
        name: "checkstyle".into(),
        image: info.image.clone(),
        commands: vec![cmd],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.config_files.clone(),
        }),
        ..Default::default()
    }
}
