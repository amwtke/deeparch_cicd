use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::strategy::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    let cmd = match &info.subdir {
        Some(subdir) => format!("cd {} && ./gradlew spotbugsMain", subdir),
        None => "./gradlew spotbugsMain".into(),
    };
    StepDef {
        name: "spotbugs".into(),
        image: info.image.clone(),
        commands: vec![cmd],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
