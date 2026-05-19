//! Shared error types for the application boundary.

/// Marker error for failures whose user-facing message has already been
/// emitted (e.g. the structured remediation guidance from `detect_backend`).
///
/// Paths that print their own actionable guidance return this instead of
/// calling [`std::process::exit`], so RAII guards still unwind. `main`
/// recognizes it by downcast and exits non-zero without printing anything
/// further, avoiding a duplicate, less helpful message.
#[derive(Debug)]
pub struct Silent;

impl std::fmt::Display for Silent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("error already reported")
    }
}

impl std::error::Error for Silent {}

/// Split an anyhow context chain into logical lines.
///
/// Every link gets its own entry: the outermost context first (so a caller
/// can continue an existing line with it), each further context next, then
/// the root cause. A context line ends with a trailing colon so it reads as
/// "caused by"; a multi-line link contributes one entry per line. No
/// indentation or framing is applied -- that is the caller's job (see
/// [`format_chain`] and [`log_error_chain`]).
fn chain_lines(error: &anyhow::Error) -> Vec<String> {
    let chain: Vec<String> = error.chain().map(|link| link.to_string()).collect();
    let (contexts, root) = chain.split_at(chain.len() - 1);

    let mut joined = String::new();
    for context in contexts {
        joined.push_str(context);
        joined.push_str(":\n");
    }
    joined.push_str(&root[0]);

    joined.lines().map(str::to_string).collect()
}

/// Render an anyhow context chain as one block for a *terminal* error
/// (`log_error_end!`): the outermost context with no leading indent so it
/// can continue the marker line, every line after it indented two spaces so
/// further contexts and the root cause stay grouped under the error.
pub fn format_chain(error: &anyhow::Error) -> String {
    let mut lines = chain_lines(error).into_iter();
    let first = lines.next().unwrap_or_default();
    std::iter::once(first)
        .chain(lines.map(|line| format!("  {line}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Log an anyhow context chain as a *continuing* (non-terminal) error.
///
/// The outermost context is emitted with `label` on the `log_error!` line
/// (`┣[ERROR] label: ...`); every further context and the root cause is
/// emitted via `log_indented!` (one call per line) so each keeps the `┃`
/// pipe framing. Use this instead of [`format_chain`] + `log_error_end!`
/// when the error is recoverable and the log continues afterward.
pub fn log_error_chain(label: &str, error: &anyhow::Error) {
    let mut lines = chain_lines(error).into_iter();
    let first = lines.next().unwrap_or_default();
    log_error!("{label}: {first}");
    for line in lines {
        log_indented!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_format_chain_multi_context_with_multiline_root() {
        let error = anyhow!("TOML parse error\n  |\n2 | x")
            .context("Failed to parse")
            .context("Configuration failed");

        assert_eq!(
            format_chain(&error),
            "Configuration failed:\n  Failed to parse:\n  TOML parse error\n    |\n  2 | x"
        );
    }

    #[test]
    fn test_format_chain_single_context_then_root() {
        let error = anyhow!("root cause").context("wrapper");
        assert_eq!(format_chain(&error), "wrapper:\n  root cause");
    }

    #[test]
    fn test_format_chain_no_context_indents_continuation() {
        let error = anyhow!("only error\nsecond line");
        assert_eq!(format_chain(&error), "only error\n  second line");
    }

    #[test]
    fn test_chain_lines_splits_context_and_root() {
        let error = anyhow!("TOML parse error\n  |\n2 | x")
            .context("Failed to parse")
            .context("Configuration failed");
        assert_eq!(
            chain_lines(&error),
            vec![
                "Configuration failed:",
                "Failed to parse:",
                "TOML parse error",
                "  |",
                "2 | x",
            ]
        );
    }

    #[test]
    fn test_chain_lines_no_context_single_link() {
        let error = anyhow!("just a root");
        assert_eq!(chain_lines(&error), vec!["just a root"]);
    }

    #[test]
    fn test_chain_lines_single_context_then_root() {
        let error = anyhow!("root cause").context("wrapper");
        assert_eq!(chain_lines(&error), vec!["wrapper:", "root cause"]);
    }
}
