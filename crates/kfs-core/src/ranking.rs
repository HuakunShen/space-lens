//! Launcher-style ranking for file search candidates.
//!
//! The ranker favors filename intent over generic full-path matches: exact
//! basename, prefix, token, and path-component matches score above substring
//! fallbacks.

use std::path::Path;

use crate::model::{lowercase_extension, MatchKind, SearchCandidate, SearchQuery};

pub fn score_candidate(
    query: &SearchQuery,
    candidate: &SearchCandidate,
    root_priority: i32,
) -> (i32, Vec<MatchKind>) {
    let query_text = query.query.trim().to_ascii_lowercase();
    let query_tokens = tokenize(&query_text);
    let mut matches = Vec::new();
    let mut score = 0;

    if query_text.is_empty() {
        matches.push(MatchKind::Empty);
        score += 1;
    } else {
        let basename = lower_file_name(&candidate.path);
        let stem = lower_file_stem(&candidate.path);
        let basename_tokens = tokenize(&basename);
        let path_components = lower_path_components(&candidate.path);

        if basename == query_text || stem == query_text {
            score += 1000;
            matches.push(MatchKind::ExactBasename);
        }

        if basename.starts_with(&query_text) || stem.starts_with(&query_text) {
            score += 800;
            matches.push(MatchKind::BasenamePrefix);
        }

        if !query_tokens.is_empty()
            && query_tokens.iter().all(|query_token| {
                basename_tokens
                    .iter()
                    .any(|basename_token| basename_token.starts_with(query_token))
            })
        {
            score += 600;
            matches.push(MatchKind::BasenameToken);
        }

        if !query_tokens.is_empty()
            && query_tokens.iter().all(|query_token| {
                path_components
                    .iter()
                    .any(|component| component.starts_with(query_token))
            })
        {
            score += 300;
            matches.push(MatchKind::PathComponent);
        }

        if !query_text.is_empty() {
            let compact_query = query_text.replace(' ', "");
            let compact_path = candidate
                .path
                .to_string_lossy()
                .to_ascii_lowercase()
                .replace([' ', '-', '_', '.', '/'], "");
            if compact_path.contains(&compact_query) {
                score += 100;
                matches.push(MatchKind::Substring);
            }
        }

        if let Some(extension) = lowercase_extension(&candidate.path) {
            if query_tokens.iter().any(|token| token == &extension) {
                score += 150;
                matches.push(MatchKind::Extension);
            }
        }
    }

    if root_priority != 0 {
        score += root_priority * 10;
        matches.push(MatchKind::RootPriority);
    }

    (score, matches)
}

pub fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn lower_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn lower_file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn lower_path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(|component| component.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{EntryKind, SearchCandidate, SearchQuery};

    use super::*;

    fn candidate(path: &str) -> SearchCandidate {
        SearchCandidate {
            path: PathBuf::from(path),
            kind: EntryKind::File,
            provider: "test".to_string(),
        }
    }

    #[test]
    fn exact_basename_scores_above_prefix() {
        let query = SearchQuery::new("main");
        let (exact, exact_matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/main.rs"), 0);
        let (prefix, prefix_matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/mainland.rs"), 0);

        assert!(exact > prefix);
        assert!(exact_matches.contains(&MatchKind::ExactBasename));
        assert!(prefix_matches.contains(&MatchKind::BasenamePrefix));
    }

    #[test]
    fn token_match_handles_package_json_query() {
        let query = SearchQuery::new("package json");
        let (score, matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/app/package.json"), 0);

        assert!(score >= 600);
        assert!(matches.contains(&MatchKind::BasenameToken));
        assert!(matches.contains(&MatchKind::Extension));
    }

    #[test]
    fn path_component_match_scores_below_basename_token() {
        let query = SearchQuery::new("docs");
        let (path_score, path_matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/docs/readme.md"), 0);
        let (name_score, name_matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/app/docs.md"), 0);

        assert!(name_score > path_score);
        assert!(path_matches.contains(&MatchKind::PathComponent));
        assert!(name_matches.contains(&MatchKind::BasenameToken));
    }

    #[test]
    fn substring_fallback_is_low_but_nonzero() {
        let query = SearchQuery::new("srclib");
        let (score, matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/app/src/lib.rs"), 0);

        assert_eq!(score, 100);
        assert!(matches.contains(&MatchKind::Substring));
    }

    #[test]
    fn root_priority_boosts_score() {
        let query = SearchQuery::new("readme");
        let (plain, _) = score_candidate(&query, &candidate("/Users/alice/Dev/readme.md"), 0);
        let (boosted, matches) =
            score_candidate(&query, &candidate("/Users/alice/Dev/readme.md"), 10);

        assert_eq!(boosted - plain, 100);
        assert!(matches.contains(&MatchKind::RootPriority));
    }
}
