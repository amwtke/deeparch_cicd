use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use super::{ProjectDetector, ProjectInfo, ProjectType};

pub struct MavenDetector;

impl MavenDetector {
    /// Normalize legacy Java version strings: "1.8" → "8", "1.7" → "7".
    /// Modern versions like "17" or "21" pass through unchanged.
    fn normalize_java_version(version: &str) -> String {
        if let Some(minor) = version.strip_prefix("1.") {
            minor.to_string()
        } else {
            version.to_string()
        }
    }

    /// Extract JDK version from pom.xml content using priority order:
    /// 1. <java.version>
    /// 2. <maven.compiler.source>
    /// 3. <maven.compiler.release>
    /// 4. Default "17"
    fn extract_jdk_version(content: &str) -> String {
        let patterns = [
            r"<java\.version>\s*([\d.]+)\s*</java\.version>",
            r"<maven\.compiler\.source>\s*([\d.]+)\s*</maven\.compiler\.source>",
            r"<maven\.compiler\.release>\s*([\d.]+)\s*</maven\.compiler\.release>",
        ];

        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(content) {
                    if let Some(version) = caps.get(1) {
                        return Self::normalize_java_version(version.as_str());
                    }
                }
            }
        }

        "17".to_string()
    }

    /// Extract Spring Boot version from pom.xml content.
    /// Looks for spring-boot-starter-parent in <parent> or
    /// spring-boot-dependencies in <dependencyManagement>.
    fn extract_spring_boot_version(content: &str) -> Option<String> {
        // Check for spring-boot-starter-parent in <parent>
        let parent_re = Regex::new(
            r"(?s)<parent>.*?<artifactId>\s*spring-boot-starter-parent\s*</artifactId>.*?<version>\s*([^<\s]+)\s*</version>.*?</parent>"
        ).ok()?;
        if let Some(caps) = parent_re.captures(content) {
            return caps.get(1).map(|m| m.as_str().to_string());
        }

        // Check for spring-boot-dependencies in <dependencyManagement>
        let dep_mgmt_re = Regex::new(
            r"(?s)<dependencyManagement>.*?<artifactId>\s*spring-boot-dependencies\s*</artifactId>.*?<version>\s*([^<\s]+)\s*</version>.*?</dependencyManagement>"
        ).ok()?;
        if let Some(caps) = dep_mgmt_re.captures(content) {
            return caps.get(1).map(|m| m.as_str().to_string());
        }

        None
    }

    /// Check if pom.xml contains the maven-checkstyle-plugin
    fn has_checkstyle(content: &str) -> bool {
        content.contains("maven-checkstyle-plugin")
    }

    /// Map JDK version string to the nearest supported Docker image.
    fn jdk_to_image(version: &str) -> String {
        let v: u32 = version.parse().unwrap_or(17);
        let mapped = if v <= 8 {
            8
        } else if v <= 11 {
            11
        } else if v <= 17 {
            17
        } else {
            21
        };
        format!("maven:3.9-eclipse-temurin-{}", mapped)
    }
}

impl ProjectDetector for MavenDetector {
    fn detect(&self, dir: &Path) -> bool {
        dir.join("pom.xml").exists()
    }

    fn analyze(&self, dir: &Path) -> Result<ProjectInfo> {
        let pom_path = dir.join("pom.xml");
        let content = fs::read_to_string(&pom_path)?;

        let jdk_version = Self::extract_jdk_version(&content);
        let spring_boot_version = Self::extract_spring_boot_version(&content);
        let image = Self::jdk_to_image(&jdk_version);

        let mut warnings = Vec::new();

        // Warn if Spring Boot 3.x with JDK < 17
        if let Some(ref sb_version) = spring_boot_version {
            if sb_version.starts_with("3.") {
                let jdk_num: u32 = jdk_version.parse().unwrap_or(17);
                if jdk_num < 17 {
                    warnings.push(format!(
                        "Spring Boot {} requires JDK 17+, but JDK {} is configured",
                        sb_version, jdk_version
                    ));
                }
            }
        }

        let framework = spring_boot_version.map(|v| format!("spring-boot {}", v));

        let lint_cmd = if Self::has_checkstyle(&content) {
            Some(vec!["mvn checkstyle:check".to_string()])
        } else {
            None
        };

        Ok(ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some(jdk_version),
            framework,
            image,
            build_cmd: vec!["mvn compile -q".to_string()],
            test_cmd: vec!["mvn test".to_string()],
            lint_cmd,
            fmt_cmd: None,
            source_paths: vec![
                "src/main/java/".to_string(),
                "src/main/resources/".to_string(),
            ],
            config_files: vec!["pom.xml".to_string()],
            warnings,
            subdir: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_maven_project() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pom.xml"), "<project></project>").unwrap();
        let detector = MavenDetector;
        assert!(detector.detect(dir.path()));
    }

    #[test]
    fn test_detect_non_maven_project() {
        let dir = tempfile::tempdir().unwrap();
        let detector = MavenDetector;
        assert!(!detector.detect(dir.path()));
    }

    #[test]
    fn test_analyze_jdk_version() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <properties>
    <java.version>21</java.version>
  </properties>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("21".into()));
        assert!(info.image.contains("21"));
    }

    #[test]
    fn test_analyze_spring_boot_version() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <parent>
    <groupId>org.springframework.boot</groupId>
    <artifactId>spring-boot-starter-parent</artifactId>
    <version>3.2.0</version>
  </parent>
  <properties>
    <java.version>17</java.version>
  </properties>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.framework, Some("spring-boot 3.2.0".into()));
        assert_eq!(info.language_version, Some("17".into()));
    }

    #[test]
    fn test_spring_boot_3_with_old_jdk_warns() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <parent>
    <groupId>org.springframework.boot</groupId>
    <artifactId>spring-boot-starter-parent</artifactId>
    <version>3.1.0</version>
  </parent>
  <properties>
    <java.version>11</java.version>
  </properties>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert!(!info.warnings.is_empty());
        assert!(info.warnings[0].contains("17"));
    }

    #[test]
    fn test_analyze_legacy_jdk_version_1_8() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <properties>
    <java.version>1.8</java.version>
  </properties>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("8".into()));
        assert!(info.image.contains("8"));
    }

    #[test]
    fn test_analyze_legacy_compiler_source_1_7() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <properties>
    <maven.compiler.source>1.7</maven.compiler.source>
  </properties>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("7".into()));
    }

    #[test]
    fn test_default_jdk_version() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"<project><modelVersion>4.0.0</modelVersion></project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("17".into()));
    }

    #[test]
    fn test_checkstyle_detection() {
        let dir = tempfile::tempdir().unwrap();
        let pom = r#"
<project>
  <properties><java.version>17</java.version></properties>
  <build><plugins><plugin>
    <artifactId>maven-checkstyle-plugin</artifactId>
  </plugin></plugins></build>
</project>"#;
        fs::write(dir.path().join("pom.xml"), pom).unwrap();
        let detector = MavenDetector;
        let info = detector.analyze(dir.path()).unwrap();
        assert!(info.lint_cmd.is_some());
    }
}
