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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_output_mode_json() {
        assert_eq!(resolve_output_mode(Some("json".into())), OutputMode::Json);
    }

    #[test]
    fn test_resolve_output_mode_plain() {
        assert_eq!(resolve_output_mode(Some("plain".into())), OutputMode::Plain);
    }

    #[test]
    fn test_resolve_output_mode_tty() {
        assert_eq!(resolve_output_mode(Some("tty".into())), OutputMode::Tty);
    }

    #[test]
    fn test_resolve_output_mode_none_defaults() {
        // When None, should auto-detect (will be Plain in test environment since not a TTY)
        let mode = resolve_output_mode(None);
        assert!(mode == OutputMode::Tty || mode == OutputMode::Plain);
    }

    #[test]
    fn test_resolve_output_mode_unknown_defaults() {
        // Unknown value should auto-detect
        let mode = resolve_output_mode(Some("unknown".into()));
        assert!(mode == OutputMode::Tty || mode == OutputMode::Plain);
    }
}
