use crate::ci::detector::ProjectInfo;
use crate::ci::parser::{OnFailure, Strategy};
use crate::ci::builder::{PipelineStrategy, StepDef};

pub const GIT_PULL_STEP_NAME: &str = "git-pull";
pub const GIT_PULL_IMAGE: &str = "alpine/git:latest";

pub struct BaseStrategy;

impl BaseStrategy {
    /// Fixed first step: pull latest code from remote git repository.
    /// - If not a git repo → skip (exit 0)
    /// - If no remote configured → skip (exit 0)
    /// - If remote exists → git pull --ff-only; conflict → error (exit 1)
    pub fn git_pull_step() -> StepDef {
        StepDef {
            name: GIT_PULL_STEP_NAME.into(),
            image: GIT_PULL_IMAGE.into(),
            commands: vec![
                "if [ ! -d .git ]; then echo 'Not a git repository, skipping'; exit 0; fi".into(),
                "if ! git remote | grep -q .; then echo 'No remote configured, skipping'; exit 0; fi".into(),
                "echo \"Pulling from $(git remote get-url origin 2>/dev/null || git remote get-url $(git remote | head -1))...\"".into(),
                "STASHED=false; if ! git diff --quiet || ! git diff --cached --quiet; then echo 'Stashing local changes...'; git stash && STASHED=true; fi".into(),
                "git pull --rebase || { if $STASHED; then git stash pop; fi; echo 'ERROR: git pull --rebase failed — possible merge conflict'; exit 1; }".into(),
                "if $STASHED; then echo 'Restoring stashed changes...'; git stash pop || { echo 'ERROR: stash pop conflict — run git stash pop manually'; exit 1; }; fi".into(),
            ],
            volumes: vec![
                "~/.ssh:/root/.ssh:ro".into(),
                "~/.gitconfig:/root/.gitconfig:ro".into(),
            ],
            on_failure: Some(OnFailure {
                strategy: Strategy::Abort,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    pub fn build_step(info: &ProjectInfo) -> StepDef {
        StepDef {
            name: "build".into(),
            image: info.image.clone(),
            commands: info.build_cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 3,
                context_paths: [&info.source_paths[..], &info.config_files[..]].concat(),
            }),
            ..Default::default()
        }
    }

    pub fn test_step(info: &ProjectInfo) -> StepDef {
        StepDef {
            name: "test".into(),
            image: info.image.clone(),
            commands: info.test_cmd.clone(),
            depends_on: vec!["build".into()],
            on_failure: Some(OnFailure {
                strategy: Strategy::Notify,
                max_retries: 0,
                context_paths: vec![],
            }),
            ..Default::default()
        }
    }

    pub fn lint_step(info: &ProjectInfo) -> Option<StepDef> {
        info.lint_cmd.as_ref().map(|cmd| StepDef {
            name: "lint".into(),
            image: info.image.clone(),
            commands: cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 2,
                context_paths: info.source_paths.clone(),
            }),
            ..Default::default()
        })
    }

    pub fn fmt_step(info: &ProjectInfo) -> Option<StepDef> {
        info.fmt_cmd.as_ref().map(|cmd| StepDef {
            name: "fmt-check".into(),
            image: info.image.clone(),
            commands: cmd.clone(),
            on_failure: Some(OnFailure {
                strategy: Strategy::AutoFix,
                max_retries: 1,
                context_paths: info.source_paths.clone(),
            }),
            ..Default::default()
        })
    }
}

pub struct BaseOnlyStrategy;

impl PipelineStrategy for BaseOnlyStrategy {
    fn pipeline_name(&self, info: &ProjectInfo) -> String {
        format!(
            "{}-ci",
            format!("{}", info.project_type)
                .to_lowercase()
                .replace('/', "-")
        )
    }

    fn steps(&self, info: &ProjectInfo) -> Vec<StepDef> {
        let mut steps = vec![BaseStrategy::build_step(info)];
        if let Some(lint) = BaseStrategy::lint_step(info) {
            steps.push(lint);
        }
        steps.push(BaseStrategy::test_step(info));
        if let Some(fmt) = BaseStrategy::fmt_step(info) {
            steps.push(fmt);
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci::detector::{ProjectInfo, ProjectType};
    use crate::ci::parser::Strategy;

    fn make_info_full() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Rust,
            language_version: Some("1.78".into()),
            framework: None,
            image: "rust:1.78-slim".into(),
            build_cmd: vec!["cargo build".into()],
            test_cmd: vec!["cargo test".into()],
            lint_cmd: Some(vec!["cargo clippy -- -D warnings".into()]),
            fmt_cmd: Some(vec!["cargo fmt -- --check".into()]),
            source_paths: vec!["src/".into()],
            config_files: vec!["Cargo.toml".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    fn make_info_minimal() -> ProjectInfo {
        ProjectInfo {
            project_type: ProjectType::Go,
            language_version: Some("1.22".into()),
            framework: None,
            image: "golang:1.22".into(),
            build_cmd: vec!["go build ./...".into()],
            test_cmd: vec!["go test ./...".into()],
            lint_cmd: None,
            fmt_cmd: None,
            source_paths: vec![".".into()],
            config_files: vec!["go.mod".into()],
            warnings: vec![],
            quality_plugins: vec![],
            subdir: None,
        }
    }

    #[test]
    fn test_git_pull_step_defaults() {
        let step = BaseStrategy::git_pull_step();
        assert_eq!(step.name, "git-pull");
        assert_eq!(step.image, "alpine/git:latest");
        assert!(step.depends_on.is_empty());
        assert_eq!(step.workdir, "/workspace");
        // Should have SSH and gitconfig volume mounts
        assert!(step.volumes.iter().any(|v| v.contains(".ssh")));
        assert!(step.volumes.iter().any(|v| v.contains(".gitconfig")));
        // Commands should handle: no git, no remote, pull
        assert!(step.commands.len() >= 3);
        assert!(step.commands[0].contains(".git"));
        assert!(step.commands[1].contains("git remote"));
        // On failure should abort
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::Abort);
        assert_eq!(on_failure.max_retries, 0);
    }

    #[test]
    fn test_build_step_defaults() {
        let info = make_info_full();
        let step = BaseStrategy::build_step(&info);
        assert_eq!(step.name, "build");
        assert_eq!(step.image, "rust:1.78-slim");
        assert_eq!(step.commands, vec!["cargo build"]);
        assert!(step.depends_on.is_empty());
        assert_eq!(step.workdir, "/workspace");
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::AutoFix);
        assert_eq!(on_failure.max_retries, 3);
        // context_paths includes source_paths + config_files
        assert!(on_failure.context_paths.contains(&"src/".to_string()));
        assert!(on_failure.context_paths.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_test_step_depends_on_build() {
        let info = make_info_full();
        let step = BaseStrategy::test_step(&info);
        assert_eq!(step.name, "test");
        assert_eq!(step.depends_on, vec!["build"]);
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::Notify);
        assert_eq!(on_failure.max_retries, 0);
    }

    #[test]
    fn test_lint_step_returns_none_when_no_lint_cmd() {
        let info = make_info_minimal();
        assert!(BaseStrategy::lint_step(&info).is_none());
    }

    #[test]
    fn test_lint_step_returns_some() {
        let info = make_info_full();
        let step = BaseStrategy::lint_step(&info).unwrap();
        assert_eq!(step.name, "lint");
        let on_failure = step.on_failure.as_ref().unwrap();
        assert_eq!(on_failure.strategy, Strategy::AutoFix);
        assert_eq!(on_failure.max_retries, 2);
    }

    #[test]
    fn test_fmt_step_returns_none_when_no_fmt_cmd() {
        let info = make_info_minimal();
        assert!(BaseStrategy::fmt_step(&info).is_none());
    }

    #[test]
    fn test_base_only_strategy_full_steps() {
        let info = make_info_full();
        let strategy = BaseOnlyStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "lint");
        assert_eq!(steps[2].name, "test");
        assert_eq!(steps[3].name, "fmt-check");
    }

    #[test]
    fn test_base_only_strategy_minimal_steps() {
        let info = make_info_minimal();
        let strategy = BaseOnlyStrategy;
        let steps = strategy.steps(&info);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "build");
        assert_eq!(steps[1].name, "test");
    }

    #[test]
    fn test_base_only_pipeline_name() {
        let info = make_info_full();
        let strategy = BaseOnlyStrategy;
        assert_eq!(strategy.pipeline_name(&info), "rust-ci");
    }
}
