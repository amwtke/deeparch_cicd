use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use super::{ProjectDetector, ProjectInfo, ProjectType};

pub struct PythonDetector;

impl PythonDetector {
    /// Extract Python version from pyproject.toml content.
    /// Looks for `python_requires = ">=3.12"` or `requires-python = ">=3.12"`.
    fn extract_python_version(content: &str) -> Option<String> {
        let patterns = [
            r#"python_requires\s*=\s*">=?(\d+\.\d+)"#,
            r#"requires-python\s*=\s*">=?(\d+\.\d+)"#,
        ];
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(content) {
                    if let Some(version) = caps.get(1) {
                        return Some(version.as_str().to_string());
                    }
                }
            }
        }
        None
    }

    /// Check if a framework name appears in content (requirements.txt or pyproject.toml).
    fn detect_framework(content: &str) -> Option<String> {
        let frameworks = ["django", "flask", "fastapi"];
        for framework in &frameworks {
            if content.to_lowercase().contains(framework) {
                return Some(framework.to_string());
            }
        }
        None
    }

    /// Check if pytest appears in the content.
    fn has_pytest(content: &str) -> bool {
        content.contains("pytest")
    }

    /// Check if ruff appears in the content.
    fn has_ruff(content: &str) -> bool {
        content.contains("ruff")
    }
}

impl ProjectDetector for PythonDetector {
    fn detect(&self, dir: &Path) -> bool {
        dir.join("pyproject.toml").exists()
            || dir.join("requirements.txt").exists()
            || dir.join("setup.py").exists()
    }

    fn analyze(&self, dir: &Path) -> Result<ProjectInfo> {
        let has_pyproject = dir.join("pyproject.toml").exists();
        let has_requirements = dir.join("requirements.txt").exists();

        // Read all available content for analysis
        let mut combined_content = String::new();
        let mut config_files = Vec::new();

        if has_pyproject {
            let content = fs::read_to_string(dir.join("pyproject.toml"))?;
            combined_content.push_str(&content);
            config_files.push("pyproject.toml".to_string());
        }
        if has_requirements {
            let content = fs::read_to_string(dir.join("requirements.txt"))?;
            combined_content.push_str(&content);
            config_files.push("requirements.txt".to_string());
        }
        if dir.join("setup.py").exists() && config_files.is_empty() {
            config_files.push("setup.py".to_string());
        }

        // Extract python version (only from pyproject.toml)
        let python_version = if has_pyproject {
            let pyproject_content = fs::read_to_string(dir.join("pyproject.toml"))?;
            Self::extract_python_version(&pyproject_content)
                .unwrap_or_else(|| "3.12".to_string())
        } else {
            "3.12".to_string()
        };

        let framework = Self::detect_framework(&combined_content);
        let has_pytest = Self::has_pytest(&combined_content);
        let has_ruff = Self::has_ruff(&combined_content);

        let image = format!("python:{}-slim", python_version);

        let build_cmd = if has_requirements {
            vec!["pip install -r requirements.txt".to_string()]
        } else {
            vec!["pip install .".to_string()]
        };

        let test_cmd = if has_pytest {
            vec!["pytest".to_string()]
        } else {
            vec!["python -m unittest discover".to_string()]
        };

        let lint_cmd = if has_ruff {
            Some(vec!["ruff check .".to_string()])
        } else {
            None
        };

        let fmt_cmd = if has_ruff {
            Some(vec!["ruff format --check .".to_string()])
        } else {
            None
        };

        Ok(ProjectInfo {
            project_type: ProjectType::Python,
            language_version: Some(python_version),
            framework,
            image,
            build_cmd,
            test_cmd,
            lint_cmd,
            fmt_cmd,
            source_paths: vec!["src/".to_string(), "app/".to_string()],
            config_files,
            warnings: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_with_requirements_txt() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask==2.0.0\n").unwrap();
        let detector = PythonDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_with_pyproject_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"myapp\"\n").unwrap();
        let detector = PythonDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_with_setup_py() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("setup.py"), "from setuptools import setup\n").unwrap();
        let detector = PythonDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_non_python_project() {
        let dir = tempfile::tempdir().unwrap();
        let detector = PythonDetector;
        assert!(!detector.detect(dir.path()));
    }

    #[test]
    fn test_analyze_python_version_from_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        let pyproject = r#"
[project]
name = "myapp"
requires-python = ">=3.11"
"#;
        fs::write(dir.path().join("pyproject.toml"), pyproject).unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("3.11".into()));
        assert!(info.image.contains("3.11"));
    }

    #[test]
    fn test_analyze_python_requires_syntax() {
        let dir = tempfile::tempdir().unwrap();
        let pyproject = r#"
[options]
python_requires = ">=3.10"
"#;
        fs::write(dir.path().join("pyproject.toml"), pyproject).unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("3.10".into()));
    }

    #[test]
    fn test_analyze_default_python_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "requests==2.28.0\n").unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("3.12".into()));
        assert_eq!(info.image, "python:3.12-slim");
    }

    #[test]
    fn test_analyze_framework_detection() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "fastapi==0.100.0\nuvicorn==0.23.0\n").unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.framework, Some("fastapi".into()));
    }

    #[test]
    fn test_analyze_pytest_detected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "pytest==7.4.0\nrequests==2.28.0\n").unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.test_cmd, vec!["pytest"]);
    }

    #[test]
    fn test_analyze_ruff_detected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "ruff==0.1.0\n").unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.lint_cmd, Some(vec!["ruff check .".to_string()]));
        assert_eq!(info.fmt_cmd, Some(vec!["ruff format --check .".to_string()]));
    }

    #[test]
    fn test_analyze_build_cmd_requirements() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "django==4.2.0\n").unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.build_cmd, vec!["pip install -r requirements.txt"]);
    }

    #[test]
    fn test_analyze_build_cmd_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"myapp\"\n").unwrap();
        let detector = PythonDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.build_cmd, vec!["pip install ."]);
    }
}
