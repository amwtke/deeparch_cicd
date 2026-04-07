pub mod base;
pub mod maven;
pub mod gradle;
pub mod rust_lang;
pub mod node;
pub mod python;
pub mod go;
pub mod test_parser;

use std::collections::HashMap;

use crate::detector::{ProjectInfo, ProjectType};
use crate::pipeline::{OnFailure, Pipeline, Step};
use crate::pipeline_gen::test_parser::TestSummary;

/// Definition of a single pipeline step, produced by strategies.
#[derive(Debug, Clone)]
pub struct StepDef {
    pub name: String,
    pub image: String,
    pub commands: Vec<String>,
    pub depends_on: Vec<String>,
    pub workdir: String,
    pub on_failure: Option<OnFailure>,
    pub allow_failure: bool,
    pub volumes: Vec<String>,
}

impl Default for StepDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            commands: vec![],
            depends_on: vec![],
            workdir: "/workspace".into(),
            on_failure: None,
            allow_failure: false,
            volumes: vec![],
        }
    }
}

impl From<StepDef> for Step {
    fn from(sd: StepDef) -> Self {
        Step {
            name: sd.name,
            image: sd.image,
            commands: sd.commands,
            depends_on: sd.depends_on,
            workdir: sd.workdir,
            on_failure: sd.on_failure,
            allow_failure: sd.allow_failure,
            volumes: sd.volumes,
            env: HashMap::new(),
            condition: None,
        }
    }
}

/// All language strategies implement this trait.
pub trait PipelineStrategy {
    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef>;
    fn pipeline_name(&self, info: &ProjectInfo) -> String;
    fn parse_test_output(&self, _output: &str) -> Option<TestSummary> {
        None
    }
}

fn strategy_for(project_type: &ProjectType) -> Box<dyn PipelineStrategy> {
    match project_type {
        ProjectType::Maven => Box::new(maven::MavenStrategy),
        ProjectType::Gradle => Box::new(gradle::GradleStrategy),
        ProjectType::Rust => Box::new(rust_lang::RustStrategy),
        ProjectType::Node => Box::new(node::NodeStrategy),
        ProjectType::Python => Box::new(python::PythonStrategy),
        ProjectType::Go => Box::new(go::GoStrategy),
    }
}

/// Get strategy by pipeline name prefix (for parsing test output after pipeline execution).
/// Returns None if no matching strategy is found.
pub fn strategy_for_pipeline(pipeline: &Pipeline) -> Option<Box<dyn PipelineStrategy>> {
    let name = &pipeline.name;
    if name.starts_with("maven") {
        Some(Box::new(maven::MavenStrategy))
    } else if name.starts_with("gradle") {
        Some(Box::new(gradle::GradleStrategy))
    } else if name.starts_with("rust") {
        Some(Box::new(rust_lang::RustStrategy))
    } else if name.starts_with("node") {
        Some(Box::new(node::NodeStrategy))
    } else if name.starts_with("python") {
        Some(Box::new(python::PythonStrategy))
    } else if name.starts_with("go") {
        Some(Box::new(go::GoStrategy))
    } else {
        None
    }
}

/// Generate a Pipeline from ProjectInfo using the strategy system.
pub fn generate_pipeline(info: &ProjectInfo) -> Pipeline {
    let strategy = strategy_for(&info.project_type);
    let step_defs = strategy.steps(info);
    let name = strategy.pipeline_name(info);

    Pipeline {
        name,
        env: HashMap::new(),
        steps: step_defs.into_iter().map(|sd| sd.into()).collect(),
    }
}
