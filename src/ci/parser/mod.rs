use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Top-level pipeline definition, maps directly from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub name: String,

    /// Global environment variables available to all steps
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// Pipeline steps
    pub steps: Vec<Step>,
}

/// A single step in the pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Unique step name
    pub name: String,

    /// Docker image to use
    pub image: String,

    /// Commands to execute inside the container
    #[serde(default)]
    pub commands: Vec<String>,

    /// Steps that must complete before this one
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,

    /// Step-level environment variables (merged with global)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// Working directory inside the container
    #[serde(default = "default_workdir", skip_serializing_if = "is_default_workdir")]
    pub workdir: String,

    /// Continue pipeline even if this step fails
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_failure: bool,

    /// Conditional execution expression
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    /// Failure handling configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_failure: Option<OnFailure>,

    /// Additional volume mounts (host:container format)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,
}

fn default_workdir() -> String {
    "/workspace".to_string()
}

fn is_default_workdir(s: &str) -> bool {
    s == "/workspace"
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Abort,
    AutoFix,
    Notify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnFailure {
    #[serde(default = "default_strategy")]
    pub strategy: Strategy,

    #[serde(default)]
    pub max_retries: u32,

    #[serde(default)]
    pub context_paths: Vec<String>,
}

fn default_strategy() -> Strategy {
    Strategy::Abort
}

impl Pipeline {
    /// Load and validate a pipeline from a YAML file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).context(format!("Cannot read {}", path.display()))?;
        Self::from_str(&content)
    }

    /// Parse and validate a pipeline from a YAML string
    pub fn from_str(yaml: &str) -> Result<Self> {
        let pipeline: Pipeline =
            serde_yaml::from_str(yaml).context("Failed to parse pipeline YAML")?;
        pipeline.validate()?;
        Ok(pipeline)
    }

    /// Validate pipeline integrity
    fn validate(&self) -> Result<()> {
        if self.steps.is_empty() {
            bail!("Pipeline '{}' has no steps defined", self.name);
        }

        // Check for duplicate step names
        let mut seen = HashMap::new();
        for step in &self.steps {
            if let Some(_) = seen.insert(&step.name, true) {
                bail!("Duplicate step name: '{}'", step.name);
            }
        }

        // Check that all depends_on references exist
        let step_names: HashMap<&str, bool> =
            self.steps.iter().map(|s| (s.name.as_str(), true)).collect();

        for step in &self.steps {
            for dep in &step.depends_on {
                if !step_names.contains_key(dep.as_str()) {
                    bail!(
                        "Step '{}' depends on '{}', which does not exist",
                        step.name,
                        dep
                    );
                }
            }

            // Self-dependency check
            if step.depends_on.contains(&step.name) {
                bail!("Step '{}' depends on itself", step.name);
            }
        }

        Ok(())
    }

    /// Get a step by name
    pub fn get_step(&self, name: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.name == name)
    }

    /// Get merged env for a step (global + step-level, step overrides global)
    pub fn merged_env(&self, step: &Step) -> HashMap<String, String> {
        let mut env = self.env.clone();
        env.extend(step.env.clone());
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_on_failure() {
        let yaml = r#"
name: test-pipeline
steps:
  - name: build
    image: rust:1.78
    commands:
      - cargo build
    on_failure:
      strategy: auto_fix
      max_retries: 3
      context_paths:
        - src/
        - Cargo.toml
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let step = &pipeline.steps[0];
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::AutoFix);
        assert_eq!(on_failure.max_retries, 3);
        assert_eq!(on_failure.context_paths, vec!["src/", "Cargo.toml"]);
    }

    #[test]
    fn test_on_failure_defaults() {
        let yaml = r#"
name: test-pipeline
steps:
  - name: build
    image: rust:1.78
    commands:
      - cargo build
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let step = &pipeline.steps[0];
        assert!(step.on_failure.is_none());
    }

    #[test]
    fn test_on_failure_notify_strategy() {
        let yaml = r#"
name: test-pipeline
steps:
  - name: test
    image: rust:1.78
    commands:
      - cargo test
    on_failure:
      strategy: notify
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let step = &pipeline.steps[0];
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::Notify);
        assert_eq!(on_failure.max_retries, 0);
        assert!(on_failure.context_paths.is_empty());
    }

    #[test]
    fn test_parse_basic_pipeline() {
        let yaml = r#"
name: test-pipeline
steps:
  - name: build
    image: rust:1.78
    commands:
      - cargo build

  - name: test
    image: rust:1.78
    depends_on: [build]
    commands:
      - cargo test
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        assert_eq!(pipeline.name, "test-pipeline");
        assert_eq!(pipeline.steps.len(), 2);
        assert_eq!(pipeline.steps[1].depends_on, vec!["build"]);
    }

    #[test]
    fn test_duplicate_step_name() {
        let yaml = r#"
name: bad
steps:
  - name: build
    image: rust:1.78
    commands: [echo hi]
  - name: build
    image: rust:1.78
    commands: [echo hi]
"#;
        assert!(Pipeline::from_str(yaml).is_err());
    }

    #[test]
    fn test_invalid_dependency() {
        let yaml = r#"
name: bad
steps:
  - name: build
    image: rust:1.78
    depends_on: [nonexistent]
    commands: [echo hi]
"#;
        assert!(Pipeline::from_str(yaml).is_err());
    }

    #[test]
    fn test_merged_env() {
        let yaml = r#"
name: test
env:
  GLOBAL_VAR: global_value
  SHARED: from_global
steps:
  - name: build
    image: rust:1.78
    commands: [echo hi]
    env:
      STEP_VAR: step_value
      SHARED: from_step
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let step = &pipeline.steps[0];
        let env = pipeline.merged_env(step);
        assert_eq!(env.get("GLOBAL_VAR").unwrap(), "global_value");
        assert_eq!(env.get("STEP_VAR").unwrap(), "step_value");
        assert_eq!(env.get("SHARED").unwrap(), "from_step"); // step overrides global
    }

    #[test]
    fn test_empty_pipeline_fails() {
        let yaml = r#"
name: empty
steps: []
"#;
        assert!(Pipeline::from_str(yaml).is_err());
    }

    #[test]
    fn test_self_dependency_fails() {
        let yaml = r#"
name: bad
steps:
  - name: loop
    image: rust:1.78
    depends_on: [loop]
    commands: [echo hi]
"#;
        assert!(Pipeline::from_str(yaml).is_err());
    }

    #[test]
    fn test_default_workdir() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: rust:1.78
    commands: [echo hi]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        assert_eq!(pipeline.steps[0].workdir, "/workspace");
    }

    #[test]
    fn test_allow_failure_default() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: rust:1.78
    commands: [echo hi]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        assert!(!pipeline.steps[0].allow_failure);
    }

    #[test]
    fn test_on_failure_abort_strategy() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: rust:1.78
    commands: [echo hi]
    on_failure:
      strategy: abort
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        let of = pipeline.steps[0].on_failure.as_ref().unwrap();
        assert_eq!(of.strategy, Strategy::Abort);
    }

    #[test]
    fn test_get_step_by_name() {
        let yaml = r#"
name: test
steps:
  - name: build
    image: rust:1.78
    commands: [echo build]
  - name: test
    image: rust:1.78
    commands: [echo test]
"#;
        let pipeline = Pipeline::from_str(yaml).unwrap();
        assert!(pipeline.get_step("build").is_some());
        assert!(pipeline.get_step("test").is_some());
        assert!(pipeline.get_step("nonexistent").is_none());
    }
}
