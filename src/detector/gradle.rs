use anyhow::Result;
use std::path::Path;
use super::{ProjectDetector, ProjectInfo};

pub struct GradleDetector;

impl ProjectDetector for GradleDetector {
    fn detect(&self, _dir: &Path) -> bool {
        false
    }

    fn analyze(&self, _dir: &Path) -> Result<ProjectInfo> {
        anyhow::bail!("Not implemented")
    }
}
