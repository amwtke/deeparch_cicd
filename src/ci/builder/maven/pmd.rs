use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::builder::StepDef;

pub fn step(info: &ProjectInfo) -> StepDef {
    let cd_prefix = match &info.subdir {
        Some(subdir) => format!("cd {} && ", subdir),
        None => String::new(),
    };

    // Use custom ruleset if exists, otherwise default rules
    // Report output goes to pipelight-misc/
    let cmd = format!(
        "{}if [ -f /workspace/pipelight-misc/pmd-ruleset.xml ]; then \
         mvn pmd:pmd -Dpmd.rulesetfiles=/workspace/pipelight-misc/pmd-ruleset.xml \
         -Dpmd.outputDirectory=/workspace/pipelight-misc/pmd-report; \
         else mvn pmd:pmd \
         -Dpmd.outputDirectory=/workspace/pipelight-misc/pmd-report; fi",
        cd_prefix
    );

    StepDef {
        name: "pmd".into(),
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
