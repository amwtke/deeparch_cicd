use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    let cmd = match &info.subdir {
        Some(subdir) => format!("cd {} && mvn package -DskipTests", subdir),
        None => "mvn package -DskipTests".into(),
    };
    StepDef {
        name: "package".into(),
        image: info.image.clone(),
        commands: vec![cmd],
        depends_on: vec!["test".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::Abort,
            max_retries: 0,
            context_paths: vec![],
        }),
        ..Default::default()
    }
}
