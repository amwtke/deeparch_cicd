use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "package".into(),
        image: info.image.clone(),
        commands: vec!["mvn package -DskipTests".into()],
        depends_on: vec!["test".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::Abort,
            max_retries: 0,
            context_paths: vec![],
        }),
        ..Default::default()
    }
}
