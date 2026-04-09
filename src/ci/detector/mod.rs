pub mod base;
pub mod go;
pub mod gradle;
pub mod maven;
pub mod node;
pub mod python;
pub mod rust_project;

// Re-export base types for external use
pub use base::{detect_and_generate, ProjectDetector, ProjectInfo, ProjectType};
