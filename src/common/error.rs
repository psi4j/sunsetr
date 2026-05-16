//! Shared error types for the application boundary.

/// Marker error for failures whose user-facing message has already been
/// emitted (e.g. the structured remediation guidance from `detect_backend`).
///
/// Paths that print their own actionable guidance return this instead of
/// calling [`std::process::exit`], so RAII guards still unwind. `main`
/// recognizes it by downcast and exits non-zero without printing anything
/// further, avoiding a duplicate, less helpful message.
#[derive(Debug)]
pub struct AlreadyReported;

impl std::fmt::Display for AlreadyReported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("error already reported")
    }
}

impl std::error::Error for AlreadyReported {}

/// Render an anyhow context chain as one indented error block.
///
/// The outermost context is returned with no leading indent so the caller
/// can continue an existing line with it. Each further context is placed on
/// its own line, the root cause is attached to the last context, and every
/// line after the first is indented two spaces so a multi-line root cause
/// stays visually grouped under the error.
pub fn format_chain(error: &anyhow::Error) -> String {
    let chain: Vec<String> = error.chain().map(|link| link.to_string()).collect();
    let (contexts, root) = chain.split_at(chain.len() - 1);

    let mut joined = String::new();
    for (i, context) in contexts.iter().enumerate() {
        joined.push_str(context);
        joined.push_str(if i + 1 < contexts.len() { ": \n" } else { ": " });
    }
    joined.push_str(&root[0]);

    let mut lines = joined.lines();
    let first = lines.next().unwrap_or_default().to_string();
    std::iter::once(first)
        .chain(lines.map(|line| format!("  {line}")))
        .collect::<Vec<_>>()
        .join("\n")
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
            "Configuration failed: \n  Failed to parse: TOML parse error\n    |\n  2 | x"
        );
    }

    #[test]
    fn test_format_chain_single_context_attaches_root() {
        let error = anyhow!("root cause").context("wrapper");
        assert_eq!(format_chain(&error), "wrapper: root cause");
    }

    #[test]
    fn test_format_chain_no_context_indents_continuation() {
        let error = anyhow!("only error\nsecond line");
        assert_eq!(format_chain(&error), "only error\n  second line");
    }
}
