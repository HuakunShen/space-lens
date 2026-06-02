//! Output formatting for the local `kfs` CLI.
//!
//! Formatting is kept separate from command execution so tests and future
//! adapters can reuse the same stable text and JSON-like result rendering.

use kfs_core::{ExplainResult, SearchResult};

pub fn format_results_text(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "no results".to_string();
    }
    results
        .iter()
        .map(|result| {
            format!(
                "{}\t{}\t{}",
                result.score,
                result.provider,
                result.path.to_string_lossy()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_results_json(results: &[SearchResult]) -> String {
    let items = results
        .iter()
        .map(|result| {
            format!(
                "{{\"path\":\"{}\",\"score\":{},\"provider\":\"{}\",\"matches\":[{}]}}",
                escape_json(&result.path.to_string_lossy()),
                result.score,
                escape_json(&result.provider),
                result
                    .matches
                    .iter()
                    .map(|kind| format!("\"{kind:?}\""))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{items}]")
}

pub fn format_explain_text(explain: &ExplainResult) -> String {
    let root = explain
        .root
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "path={}\nallowed={}\nroot={}\nhidden={}\nignored={}\nsensitive={}\nreasons={}",
        explain.path.to_string_lossy(),
        explain.allowed,
        root,
        explain.hidden,
        explain.ignored,
        explain.sensitive,
        explain.reasons.join(",")
    )
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kfs_core::{ExplainResult, MatchKind, SearchResult};

    use super::*;

    #[test]
    fn formats_json_results_with_escaped_paths() {
        let text = format_results_json(&[SearchResult {
            path: PathBuf::from("/Users/alice/Dev/a\"b"),
            score: 42,
            provider: "test".to_string(),
            matches: vec![MatchKind::BasenamePrefix],
        }]);

        assert_eq!(
            text,
            "[{\"path\":\"/Users/alice/Dev/a\\\"b\",\"score\":42,\"provider\":\"test\",\"matches\":[\"BasenamePrefix\"]}]"
        );
    }

    #[test]
    fn formats_explain_text() {
        let text = format_explain_text(&ExplainResult {
            path: PathBuf::from("/Users/alice/Dev/project"),
            allowed: true,
            root: Some(PathBuf::from("/Users/alice/Dev")),
            hidden: false,
            ignored: false,
            sensitive: false,
            reasons: vec!["inside-root".to_string()],
            score: None,
            matches: Vec::new(),
        });

        assert!(text.contains("allowed=true"));
        assert!(text.contains("root=/Users/alice/Dev"));
    }
}
