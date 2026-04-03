use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level pipeline definition, maps directly from YAML
#[derive(Debug, Clone, Deserialize)]
pub struct Pipeline {
    pub name: String,

    /// Global environment variables available to all steps
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Pipeline steps
    pub steps: Vec<Step>,
}

/// A single step in the pipeline
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    /// Unique step name
    pub name: String,

    /// Docker image to use
    pub image: String,

    /// Commands to execute inside the container
    #[serde(default)]
    pub commands: Vec<String>,

    /// Steps that must complete before this one
    #[serde(default)]
    pub depends_on: Vec<String>,

    /// Step-level environment variables (merged with global)
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory inside the container
    #[serde(default = "default_workdir")]
    pub workdir: String,

    /// Continue pipeline even if this step fails
    #[serde(default)]
    pub allow_failure: bool,

    /// Conditional execution expression
    #[serde(default)]
    pub condition: Option<String>,
}

fn default_workdir() -> String {
    "/workspace".to_string()
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
}
