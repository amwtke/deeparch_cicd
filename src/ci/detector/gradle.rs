use anyhow::Result;
use regex::Regex;
use std::fs;
use std::path::Path;

use super::base::{ProjectDetector, ProjectInfo, ProjectType};

pub struct GradleDetector;

impl GradleDetector {
    /// Normalize legacy Java version strings: "1.8" → "8", "1.7" → "7".
    /// Modern versions like "17" or "21" pass through unchanged.
    fn normalize_java_version(version: &str) -> String {
        if let Some(minor) = version.strip_prefix("1.") {
            minor.to_string()
        } else {
            version.to_string()
        }
    }

    /// Extract JDK version from build.gradle / build.gradle.kts content.
    /// Priority order:
    /// 1. sourceCompatibility = '17' or sourceCompatibility = 17 or '1.8'
    /// 2. JavaVersion.VERSION_17 or VERSION_1_8
    /// 3. jvmToolchain(17)
    /// 4. Default "17"
    fn extract_jdk_version(content: &str) -> String {
        let patterns = [
            r#"sourceCompatibility\s*=\s*['"]?([\d.]+)['"]?"#,
            r"JavaVersion\.VERSION_([\d_]+)",
            r"jvmToolchain\((\d+)\)",
        ];

        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(content) {
                    if let Some(version) = caps.get(1) {
                        let v = version.as_str().replace('_', ".");
                        return Self::normalize_java_version(&v);
                    }
                }
            }
        }

        "17".to_string()
    }

    /// Extract Spring Boot version from build.gradle / build.gradle.kts content.
    /// Looks for:
    /// 1. id 'org.springframework.boot' version '3.2.0'
    /// 2. org.springframework.boot:spring-boot*:3.2.0
    fn extract_spring_boot_version(content: &str) -> Option<String> {
        // Plugin block: id 'org.springframework.boot' version '3.2.0'
        let plugin_re =
            Regex::new(r#"id\s*['"]org\.springframework\.boot['"]\s*version\s*['"]([^'"]+)['"]"#)
                .ok()?;
        if let Some(caps) = plugin_re.captures(content) {
            return caps.get(1).map(|m| m.as_str().to_string());
        }

        // Dependency: org.springframework.boot:spring-boot*:3.2.0
        let dep_re =
            Regex::new(r#"org\.springframework\.boot:spring-boot[^:]*:([0-9][^\s'"]+)"#).ok()?;
        if let Some(caps) = dep_re.captures(content) {
            return caps.get(1).map(|m| m.as_str().to_string());
        }

        None
    }

    /// Check if build file contains checkstyle or pmd plugin references.
    fn has_lint(content: &str) -> bool {
        content.contains("checkstyle") || content.contains("pmd")
    }

    /// Check if build file contains spotbugs plugin
    fn has_spotbugs(content: &str) -> bool {
        content.contains("spotbugs")
    }

    /// Check if build file contains pmd plugin
    fn has_pmd(content: &str) -> bool {
        content.contains("pmd")
    }

    /// Map JDK version string to the nearest supported Gradle Docker image.
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
        format!("gradle:8-jdk{}", mapped)
    }
}

impl ProjectDetector for GradleDetector {
    fn detect(&self, dir: &Path) -> bool {
        dir.join("build.gradle").exists() || dir.join("build.gradle.kts").exists()
    }

    fn analyze(&self, dir: &Path) -> Result<ProjectInfo> {
        // Prefer build.gradle, fall back to build.gradle.kts
        let (build_file, config_file_name) = if dir.join("build.gradle").exists() {
            (dir.join("build.gradle"), "build.gradle")
        } else {
            (dir.join("build.gradle.kts"), "build.gradle.kts")
        };

        let content = fs::read_to_string(&build_file)?;

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

        let lint_cmd = if Self::has_lint(&content) {
            Some(vec!["./gradlew check -x test".to_string()])
        } else {
            None
        };

        let mut quality_plugins = Vec::new();
        if Self::has_spotbugs(&content) {
            quality_plugins.push("spotbugs".to_string());
        }
        if Self::has_pmd(&content) {
            quality_plugins.push("pmd".to_string());
        }

        Ok(ProjectInfo {
            project_type: ProjectType::Gradle,
            language_version: Some(jdk_version),
            framework,
            image,
            build_cmd: vec![
                "./gradlew assemble --max-workers=2 --build-cache --configuration-cache"
                    .to_string(),
            ],
            test_cmd: vec!["./gradlew test".to_string()],
            lint_cmd,
            fmt_cmd: None,
            source_paths: vec![
                "src/main/java/".to_string(),
                "src/main/resources/".to_string(),
            ],
            config_files: vec![config_file_name.to_string()],
            quality_plugins,
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
    fn test_detect_gradle() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'").unwrap();
        assert!(GradleDetector.detect(dir.path()));
    }

    #[test]
    fn test_detect_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle.kts"), "plugins { java }").unwrap();
        assert!(GradleDetector.detect(dir.path()));
    }

    #[test]
    fn test_detect_no_gradle() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!GradleDetector.detect(dir.path()));
    }

    #[test]
    fn test_analyze_jdk_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "sourceCompatibility = '21'",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("21".into()));
        assert!(info.image.contains("21"));
    }

    #[test]
    fn test_analyze_spring_boot() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"
plugins {
    id 'org.springframework.boot' version '3.2.0'
}
sourceCompatibility = '17'
"#;
        fs::write(dir.path().join("build.gradle"), content).unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.framework, Some("spring-boot 3.2.0".into()));
    }

    #[test]
    fn test_analyze_legacy_jdk_1_8() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "sourceCompatibility = '1.8'",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("8".into()));
        assert_eq!(info.image, "gradle:8-jdk8");
    }

    #[test]
    fn test_analyze_java_version_enum_1_8() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "sourceCompatibility = JavaVersion.VERSION_1_8",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("8".into()));
        assert_eq!(info.image, "gradle:8-jdk8");
    }

    #[test]
    fn test_analyze_default_jdk() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("17".into()));
        assert_eq!(info.image, "gradle:8-jdk17");
    }

    #[test]
    fn test_analyze_jdk_via_java_version_enum() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "sourceCompatibility = JavaVersion.VERSION_11",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("11".into()));
        assert_eq!(info.image, "gradle:8-jdk11");
    }

    #[test]
    fn test_analyze_jdk_via_toolchain() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle.kts"),
            "kotlin { jvmToolchain(21) }",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.language_version, Some("21".into()));
        assert_eq!(info.image, "gradle:8-jdk21");
    }

    #[test]
    fn test_analyze_build_cmd_uses_assemble_with_caches() {
        // The build step must not run check/test (those are separate steps), and
        // must enable Gradle's build cache + configuration cache to avoid redoing
        // work across runs. max-workers is capped to keep memory under control for
        // large multi-module projects (e.g. 100+ Spring Boot modules).
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(
            info.build_cmd,
            vec![
                "./gradlew assemble --max-workers=2 --build-cache --configuration-cache"
                    .to_string()
            ]
        );
    }

    #[test]
    fn test_analyze_lint_checkstyle() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("build.gradle"),
            "apply plugin: 'checkstyle'",
        )
        .unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(info.lint_cmd.is_some());
        assert_eq!(
            info.lint_cmd.unwrap(),
            vec!["./gradlew check -x test".to_string()]
        );
    }

    #[test]
    fn test_analyze_lint_pmd() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'pmd'").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(info.lint_cmd.is_some());
    }

    #[test]
    fn test_analyze_no_lint() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(info.lint_cmd.is_none());
    }

    #[test]
    fn test_spring_boot_3_with_old_jdk_warns() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"
plugins {
    id 'org.springframework.boot' version '3.1.0'
}
sourceCompatibility = '11'
"#;
        fs::write(dir.path().join("build.gradle"), content).unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert!(!info.warnings.is_empty());
        assert!(info.warnings[0].contains("17"));
    }

    #[test]
    fn test_config_files_kts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle.kts"), "plugins { java }").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.config_files, vec!["build.gradle.kts".to_string()]);
    }

    #[test]
    fn test_config_files_groovy_preferred() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("build.gradle"), "apply plugin: 'java'").unwrap();
        fs::write(dir.path().join("build.gradle.kts"), "plugins { java }").unwrap();
        let info = GradleDetector.analyze(dir.path()).unwrap();
        assert_eq!(info.config_files, vec!["build.gradle".to_string()]);
    }

    #[test]
    fn test_image_mapping_jdk8() {
        assert_eq!(GradleDetector::jdk_to_image("8"), "gradle:8-jdk8");
        assert_eq!(GradleDetector::jdk_to_image("6"), "gradle:8-jdk8");
    }

    #[test]
    fn test_image_mapping_jdk11() {
        assert_eq!(GradleDetector::jdk_to_image("11"), "gradle:8-jdk11");
        assert_eq!(GradleDetector::jdk_to_image("10"), "gradle:8-jdk11");
    }

    #[test]
    fn test_image_mapping_jdk17() {
        assert_eq!(GradleDetector::jdk_to_image("17"), "gradle:8-jdk17");
        assert_eq!(GradleDetector::jdk_to_image("16"), "gradle:8-jdk17");
    }

    #[test]
    fn test_image_mapping_jdk21() {
        assert_eq!(GradleDetector::jdk_to_image("21"), "gradle:8-jdk21");
        assert_eq!(GradleDetector::jdk_to_image("22"), "gradle:8-jdk21");
    }
}
