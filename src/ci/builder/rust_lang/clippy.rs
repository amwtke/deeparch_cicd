use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::builder::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    StepDef {
        name: "clippy".into(),
        image: info.image.clone(),
        commands: vec!["cargo clippy -- -D warnings".into()],
        depends_on: vec!["build".into()],
        on_failure: Some(OnFailure {
            strategy: Strategy::AutoFix,
            max_retries: 2,
            context_paths: info.source_paths.clone(),
        }),
        ..Default::default()
    }
}
