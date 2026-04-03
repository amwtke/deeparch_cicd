pub mod tty;
pub mod json;
pub mod plain;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    Tty,
    Plain,
    Json,
}

impl OutputMode {
    pub fn detect() -> Self {
        if atty::is(atty::Stream::Stdout) {
            OutputMode::Tty
        } else {
            OutputMode::Plain
        }
    }
}

/// Resolve output mode from optional CLI flag
pub fn resolve_output_mode(flag: Option<String>) -> OutputMode {
    match flag.as_deref() {
        Some("tty") => OutputMode::Tty,
        Some("plain") => OutputMode::Plain,
        Some("json") => OutputMode::Json,
        _ => OutputMode::detect(),
    }
}
