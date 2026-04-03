use anyhow::Result;
use std::path::Path;
use super::{ProjectDetector, ProjectInfo};

pub struct GoDetector;

impl ProjectDetector for GoDetector {
    fn detect(&self, _dir: &Path) -> bool {
        false
    }

    fn analyze(&self, _dir: &Path) -> Result<ProjectInfo> {
        anyhow::bail!("Not implemented")
    }
}
