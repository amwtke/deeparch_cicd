use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use super::base::{ProjectDetector, ProjectInfo, ProjectType};

pub struct RustDetector;

impl RustDetector {
    /// Extract edition from Cargo.toml content.
    /// Returns the edition string or "2021" as default.
    fn extract_edition(content: &str) -> String {
        if let Ok(re) = Regex::new(r#"edition\s*=\s*"(\d+)""#) {
            if let Some(caps) = re.captures(content) {
                if let Some(edition) = caps.get(1) {
                    return edition.as_str().to_string();
                }
            }
        }
        "2021".to_string()
    }

    /// Extract rust-version from Cargo.toml content, if present.
    fn extract_rust_version(content: &str) -> Option<String> {
        let re = Regex::new(r#"rust-version\s*=\s*"([^"]+)""#).ok()?;
        re.captures(content)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    }
}

impl ProjectDetector for RustDetector {
    fn detect(&self, dir: &Path) -> bool {
        dir.join("Cargo.toml").exists()
    }

    fn analyze(&self, dir: &Path) -> Result<ProjectInfo> {
        let cargo_toml_path = dir.join("Cargo.toml");
        let content = fs::read_to_string(&cargo_toml_path)?;

        let edition = Self::extract_edition(&content);
        let rust_version = Self::extract_rust_version(&content);

        // Language version: prefer rust-version if present, otherwise report edition
        let language_version = rust_version.clone().or_else(|| Some(edition.clone()));

        Ok(ProjectInfo {
            project_type: ProjectType::Rust,
            language_version,
            framework: None,
            image: "rust:latest".to_string(),
            build_cmd: vec!["cargo build".to_string()],
            test_cmd: vec!["cargo test".to_string()],
            lint_cmd: Some(vec![
                "rustup component add clippy 2>/dev/null; cargo clippy -- -D warnings".to_string(),
            ]),
            fmt_cmd: Some(vec![
                "rustup component add rustfmt 2>/dev/null; cargo fmt -- --check".to_string(),
            ]),
            source_paths: vec!["src/".to_string()],
            config_files: vec!["Cargo.toml".to_string()],
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
    fn test_detect_rust_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"foo\"").unwrap();
        let detector = RustDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_non_rust_project() {
        let dir = tempfile::tempdir().unwrap();
        let detector = RustDetector;
        assert!(!detector.detect(dir.path()));
    }

    #[test]
    fn test_analyze_edition() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = r#"
[package]
name = "myapp"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
        let detector = RustDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("2021".into()));
    }

    #[test]
    fn test_analyze_default_edition() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = r#"
[package]
name = "myapp"
version = "0.1.0"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
        let detector = RustDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("2021".into()));
    }

    #[test]
    fn test_analyze_with_rust_version() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = r#"
[package]
name = "myapp"
version = "0.1.0"
edition = "2021"
rust-version = "1.78"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
        let detector = RustDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("1.78".into()));
    }

    #[test]
    fn test_analyze_commands() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"foo\"\nedition = \"2021\"",
        )
        .unwrap();
        let detector = RustDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.build_cmd, vec!["cargo build"]);
        assert_eq!(info.test_cmd, vec!["cargo test"]);
        assert_eq!(
            info.lint_cmd,
            Some(vec![
                "rustup component add clippy 2>/dev/null; cargo clippy -- -D warnings".to_string()
            ])
        );
        assert_eq!(
            info.fmt_cmd,
            Some(vec![
                "rustup component add rustfmt 2>/dev/null; cargo fmt -- --check".to_string()
            ])
        );
        assert_eq!(info.image, "rust:latest");
    }
}
