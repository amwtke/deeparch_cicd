use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::pipeline_gen::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    let cd_prefix = match &info.subdir {
        Some(subdir) => format!("cd {} && ", subdir),
        None => String::new(),
    };

    let cmd = format!(
        "{}./gradlew spotbugsMain && cp -r build/reports/spotbugs /workspace/pipelight-misc/spotbugs-report 2>/dev/null || true",
        cd_prefix
    );

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
