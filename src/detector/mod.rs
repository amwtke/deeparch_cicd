pub mod base;
pub mod maven;
pub mod gradle;
pub mod rust_project;
pub mod node;
pub mod python;
pub mod go;

// Re-export base types for external use
pub use base::{ProjectDetector, ProjectInfo, ProjectType, detect_and_generate};
