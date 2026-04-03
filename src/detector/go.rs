use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use super::{ProjectDetector, ProjectInfo, ProjectType};

pub struct GoDetector;

impl GoDetector {
    /// Extract Go version from go.mod content.
    /// Looks for `go 1.22` style directive.
    fn extract_go_version(content: &str) -> String {
        if let Ok(re) = Regex::new(r"^go\s+(\d+\.\d+)", ) {
            for line in content.lines() {
                if let Some(caps) = re.captures(line.trim()) {
                    if let Some(version) = caps.get(1) {
                        return version.as_str().to_string();
                    }
                }
            }
        }
        "1.22".to_string()
    }

    /// Check if a golangci-lint config file exists.
    fn has_golangci_lint(dir: &Path) -> bool {
        dir.join(".golangci.yml").exists() || dir.join(".golangci.yaml").exists()
    }
}

impl ProjectDetector for GoDetector {
    fn detect(&self, dir: &Path) -> bool {
        dir.join("go.mod").exists()
    }

    fn analyze(&self, dir: &Path) -> Result<ProjectInfo> {
        let go_mod_path = dir.join("go.mod");
        let content = fs::read_to_string(&go_mod_path)?;

        let go_version = Self::extract_go_version(&content);
        let image = format!("golang:{}", go_version);

        let lint_cmd = if Self::has_golangci_lint(dir) {
            Some(vec!["golangci-lint run".to_string()])
        } else {
            None
        };

        Ok(ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some(go_version),
            framework: None,
            image,
            build_cmd: vec!["go build ./...".to_string()],
            test_cmd: vec!["go test ./...".to_string()],
            lint_cmd,
            fmt_cmd: Some(vec!["gofmt -l .".to_string()]),
            source_paths: vec![".".to_string()],
            config_files: vec!["go.mod".to_string()],
            warnings: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_go_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example.com/myapp\n\ngo 1.22\n").unwrap();
        let detector = GoDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_non_go_project() {
        let dir = tempfile::tempdir().unwrap();
        let detector = GoDetector;
        assert!(!detector.detect(dir.path()));
    }

    #[test]
    fn test_analyze_go_version() {
        let dir = tempfile::tempdir().unwrap();
        let go_mod = "module example.com/myapp\n\ngo 1.21\n";
        fs::write(dir.path().join("go.mod"), go_mod).unwrap();
        let detector = GoDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("1.21".into()));
        assert_eq!(info.image, "golang:1.21");
    }

    #[test]
    fn test_analyze_default_go_version() {
        let dir = tempfile::tempdir().unwrap();
        let go_mod = "module example.com/myapp\n";
        fs::write(dir.path().join("go.mod"), go_mod).unwrap();
        let detector = GoDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("1.22".into()));
        assert_eq!(info.image, "golang:1.22");
    }

    #[test]
    fn test_analyze_commands() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example.com/myapp\n\ngo 1.22\n").unwrap();
        let detector = GoDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.build_cmd, vec!["go build ./..."]);
        assert_eq!(info.test_cmd, vec!["go test ./..."]);
        assert_eq!(info.fmt_cmd, Some(vec!["gofmt -l .".to_string()]));
        assert!(info.lint_cmd.is_none()); // no golangci-lint config
    }

    #[test]
    fn test_analyze_with_golangci_lint() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example.com/myapp\n\ngo 1.22\n").unwrap();
        fs::write(dir.path().join(".golangci.yml"), "linters:\n  enable:\n    - govet\n").unwrap();
        let detector = GoDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.lint_cmd, Some(vec!["golangci-lint run".to_string()]));
    }

    #[test]
    fn test_analyze_with_golangci_yaml_extension() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example.com/myapp\n\ngo 1.22\n").unwrap();
        fs::write(dir.path().join(".golangci.yaml"), "linters:\n  enable:\n    - govet\n").unwrap();
        let detector = GoDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.lint_cmd, Some(vec!["golangci-lint run".to_string()]));
    }
}
