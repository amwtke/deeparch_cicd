use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use super::base::{ProjectDetector, ProjectInfo, ProjectType};

pub struct NodeDetector;

impl NodeDetector {
    /// Extract Node.js major version from the "engines" field in package.json.
    /// Looks for `"node": ">=18"` style entries. Returns major version number.
    fn extract_node_version(content: &str) -> Option<String> {
        let re = Regex::new(r#""node"\s*:\s*"[^"]*?(\d+)[^"]*""#).ok()?;
        re.captures(content)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// Detect framework from dependencies section.
    fn detect_framework(content: &str) -> Option<String> {
        let frameworks = ["next", "react", "vue", "angular", "express"];
        for framework in &frameworks {
            // Look for the dependency key in dependencies or devDependencies
            let pattern = format!(r#""{}"\s*:"#, framework);
            if Regex::new(&pattern).ok()?.is_match(content) {
                return Some(framework.to_string());
            }
        }
        None
    }

    /// Check if a script key exists in the "scripts" section.
    fn has_script(content: &str, script: &str) -> bool {
        let pattern = format!(r#""{}"s*:"#, regex::escape(script));
        // Use simple string search since scripts values are straightforward
        let search = format!(r#""{}""#, script);
        // Find the scripts block and check within it
        if let Some(scripts_pos) = content.find(r#""scripts""#) {
            let scripts_section = &content[scripts_pos..];
            // Find the matching closing brace for the scripts object
            if let Some(open_pos) = scripts_section.find('{') {
                let mut depth = 0;
                let section_bytes = &scripts_section.as_bytes()[open_pos..];
                let mut close_pos = open_pos;
                for (i, &b) in section_bytes.iter().enumerate() {
                    match b {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                close_pos = open_pos + i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let scripts_block = &scripts_section[open_pos..=close_pos];
                // Check for the key pattern within the scripts block
                let _ = pattern; // suppress unused warning
                return scripts_block.contains(&search);
            }
        }
        false
    }

    /// Check if the "build" script exists in package.json.
    fn has_build_script(content: &str) -> bool {
        Self::has_script(content, "build")
    }
}

impl ProjectDetector for NodeDetector {
    fn detect(&self, dir: &Path) -> bool {
        dir.join("package.json").exists()
    }

    fn analyze(&self, dir: &Path) -> Result<ProjectInfo> {
        let pkg_path = dir.join("package.json");
        let content = fs::read_to_string(&pkg_path)?;

        let node_version = Self::extract_node_version(&content).unwrap_or_else(|| "20".to_string());
        let framework = Self::detect_framework(&content);
        let image = format!("node:{}-slim", node_version);

        let build_cmd = if Self::has_build_script(&content) {
            vec!["npm install && npm run build".to_string()]
        } else {
            vec!["npm install".to_string()]
        };

        let test_cmd = if Self::has_script(&content, "test") {
            vec!["npm test".to_string()]
        } else {
            vec![]
        };

        let lint_cmd = if Self::has_script(&content, "lint") {
            Some(vec!["npm run lint".to_string()])
        } else {
            None
        };

        let fmt_cmd = if Self::has_script(&content, "format") {
            Some(vec!["npm run format".to_string()])
        } else {
            None
        };

        Ok(ProjectInfo {
            project_type: ProjectType::Node,
            language_version: Some(node_version),
            framework,
            image,
            build_cmd,
            test_cmd,
            lint_cmd,
            fmt_cmd,
            source_paths: vec!["src/".to_string()],
            config_files: vec!["package.json".to_string()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_node_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name":"app"}"#).unwrap();
        let detector = NodeDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_non_node_project() {
        let dir = tempfile::tempdir().unwrap();
        let detector = NodeDetector;
        assert!(!detector.detect(dir.path()));
    }

    #[test]
    fn test_analyze_with_engines() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{
  "name": "myapp",
  "engines": {
    "node": ">=18"
  },
  "scripts": {
    "build": "tsc",
    "test": "jest"
  }
}"#;
        fs::write(dir.path().join("package.json"), pkg).unwrap();
        let detector = NodeDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("18".into()));
        assert!(info.image.contains("18"));
        assert!(info.image.contains("slim"));
    }

    #[test]
    fn test_analyze_default_node_version() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{"name": "myapp", "scripts": {}}"#;
        fs::write(dir.path().join("package.json"), pkg).unwrap();
        let detector = NodeDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("20".into()));
        assert_eq!(info.image, "node:20-slim");
    }

    #[test]
    fn test_analyze_framework_detection_react() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{
  "name": "myapp",
  "dependencies": {
    "react": "^18.0.0",
    "react-dom": "^18.0.0"
  },
  "scripts": {
    "build": "react-scripts build",
    "test": "react-scripts test"
  }
}"#;
        fs::write(dir.path().join("package.json"), pkg).unwrap();
        let detector = NodeDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.framework, Some("react".into()));
    }

    #[test]
    fn test_analyze_framework_detection_next() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{
  "name": "myapp",
  "dependencies": {
    "next": "14.0.0"
  },
  "scripts": {
    "build": "next build"
  }
}"#;
        fs::write(dir.path().join("package.json"), pkg).unwrap();
        let detector = NodeDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.framework, Some("next".into()));
    }

    #[test]
    fn test_analyze_scripts_detection() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{
  "name": "myapp",
  "scripts": {
    "build": "tsc",
    "test": "jest",
    "lint": "eslint src/",
    "format": "prettier --check ."
  }
}"#;
        fs::write(dir.path().join("package.json"), pkg).unwrap();
        let detector = NodeDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.build_cmd, vec!["npm install && npm run build"]);
        assert_eq!(info.test_cmd, vec!["npm test"]);
        assert_eq!(info.lint_cmd, Some(vec!["npm run lint".to_string()]));
        assert_eq!(info.fmt_cmd, Some(vec!["npm run format".to_string()]));
    }

    #[test]
    fn test_analyze_no_build_script() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = r#"{"name": "myapp", "scripts": {"start": "node index.js"}}"#;
        fs::write(dir.path().join("package.json"), pkg).unwrap();
        let detector = NodeDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.build_cmd, vec!["npm install"]);
    }
}
