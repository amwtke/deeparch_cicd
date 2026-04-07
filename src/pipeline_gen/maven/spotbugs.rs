use crate::detector::ProjectInfo;
use crate::pipeline::{OnFailure, Strategy};
use crate::pipeline_gen::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    let cd_prefix = match &info.subdir {
        Some(subdir) => format!("cd {} && ", subdir),
        None => String::new(),
    };

    // Use custom exclude filter if exists
    // Report output goes to pipelight-misc/
    let cmd = format!(
        "{}if [ -f /workspace/pipelight-misc/spotbugs-exclude.xml ]; then \
         mvn spotbugs:spotbugs -Dspotbugs.excludeFilterFile=/workspace/pipelight-misc/spotbugs-exclude.xml \
         -Dspotbugs.xmlOutputDirectory=/workspace/pipelight-misc/spotbugs-report; \
         else mvn spotbugs:spotbugs \
         -Dspotbugs.xmlOutputDirectory=/workspace/pipelight-misc/spotbugs-report; fi",
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
