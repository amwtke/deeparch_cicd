pub mod checkstyle;
pub mod package;

use crate::detector::ProjectInfo;
use crate::strategy::{PipelineStrategy, StepDef};
use crate::strategy::base::BaseStrategy;

pub struct MavenStrategy;

impl PipelineStrategy for MavenStrategy {
    fn pipeline_name(&self, _info: &ProjectInfo) -> String {
        "maven-java-ci".into()
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];
        if info.lint_cmd.is_some() {
            steps.push(checkstyle::step(info));
        }
        steps.push(BaseStrategy::test_step(info));
        steps.push(package::step(info));
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::{ProjectInfo, ProjectType};

    fn make_maven_info_with_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: Some("spring-boot 3.2.0".into()),
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: Some(vec!["mvn checkstyle:check".into()]),
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
        }
    }

    fn make_maven_info_without_lint() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Maven,
            language_version: Some("17".into()),
            framework: None,
            image: "maven:3.9-eclipse-temurin-17".into(),
            build_cmd: vec!["mvn compile".into()],
            test_cmd: vec!["mvn test".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec!["src/".into()],
            config_files: vec!["pom.xml".into()],
            warnings: vec![],
        }
    }

    #[test]
    fn test_maven_steps_with_checkstyle() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "checkstyle");
        assert_eq!(steps[2].name, "test");
        assert_eq!(steps[3].name, "package");
    }

    #[test]
    fn test_maven_steps_without_checkstyle() {
        let info = make_maven_info_without_lint();
        let strategy = MavenStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "test");
        assert_eq!(steps[2].name, "package");
    }

    #[test]
    fn test_maven_pipeline_name() {
        let info = make_maven_info_with_lint();
        let strategy = MavenStrategy;
        assert_eq!(strategy.pipeline_name(&info), "maven-java-ci");
    }

    #[test]
    fn test_package_depends_on_test() {
        let info = make_maven_info_with_lint();
        let step = package::step(&info);
        assert_eq!(step.depends_on, vec!["test"]);
    }

    #[test]
    fn test_checkstyle_depends_on_build() {
        let info = make_maven_info_with_lint();
        let step = checkstyle::step(&info);
        assert_eq!(step.depends_on, vec!["build"]);
    }
}
