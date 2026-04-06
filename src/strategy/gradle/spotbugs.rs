use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "spotbugs".into(),
        image: info.image.clone(),
        commands: vec!["./gradlew spotbugsMain".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
