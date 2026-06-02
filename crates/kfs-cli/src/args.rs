//! Minimal argument parser for the local `kfs` CLI.
//!
//! This avoids adding CLI dependencies while the Rust prototype is still being
//! shaped and keeps parsing behavior easy to unit test.

use std::path::PathBuf;

use kfs_core::{SearchQuery, SearchRoot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    pub roots: Vec<SearchRoot>,
    pub command: CommandSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSpec {
    Search {
        query: SearchQuery,
        json: bool,
    },
    Explain {
        path: PathBuf,
        query: Option<SearchQuery>,
    },
    Bench {
        query: SearchQuery,
    },
}

pub fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    if args.is_empty() {
        return Err(usage());
    }

    match args[0].as_str() {
        "search" => parse_search(&args[1..]),
        "explain" => parse_explain(&args[1..]),
        "bench" => parse_bench(&args[1..]),
        _ => Err(usage()),
    }
}

fn parse_search(args: &[String]) -> Result<ParsedArgs, String> {
    let (query_text, rest) = first_value(args, "search requires a query")?;
    let mut parsed = parse_common(rest)?;
    let query = parsed.query(query_text)?;
    let json = parsed.json;
    Ok(ParsedArgs {
        roots: parsed.take_roots()?,
        command: CommandSpec::Search { query, json },
    })
}

fn parse_explain(args: &[String]) -> Result<ParsedArgs, String> {
    let (path, rest) = first_value(args, "explain requires a path")?;
    let mut parsed = parse_common(rest)?;
    let query = parsed.query_text.take().map(SearchQuery::new);
    Ok(ParsedArgs {
        roots: parsed.take_roots()?,
        command: CommandSpec::Explain {
            path: PathBuf::from(path),
            query,
        },
    })
}

fn parse_bench(args: &[String]) -> Result<ParsedArgs, String> {
    let (query_text, rest) = first_value(args, "bench requires a query")?;
    let mut parsed = parse_common(rest)?;
    let query = parsed.query(query_text)?;
    Ok(ParsedArgs {
        roots: parsed.take_roots()?,
        command: CommandSpec::Bench { query },
    })
}

fn first_value<'a>(
    args: &'a [String],
    error: &'static str,
) -> Result<(&'a str, &'a [String]), String> {
    let Some(value) = args.first() else {
        return Err(error.to_string());
    };
    if value.starts_with("--") {
        return Err(error.to_string());
    }
    Ok((value.as_str(), &args[1..]))
}

#[derive(Debug, Default)]
struct CommonArgs {
    roots: Vec<SearchRoot>,
    limit: Option<usize>,
    json: bool,
    include_hidden: bool,
    include_ignored: bool,
    extensions: Vec<String>,
    query_text: Option<String>,
}

impl CommonArgs {
    fn take_roots(&mut self) -> Result<Vec<SearchRoot>, String> {
        if self.roots.is_empty() {
            return Err("at least one --root is required".to_string());
        }
        Ok(std::mem::take(&mut self.roots))
    }

    fn query(&mut self, query_text: &str) -> Result<SearchQuery, String> {
        let mut query = SearchQuery::new(query_text);
        if let Some(limit) = self.limit {
            query.limit = limit;
        }
        query.include_hidden = self.include_hidden;
        query.include_ignored = self.include_ignored;
        query.extensions = std::mem::take(&mut self.extensions);
        Ok(query)
    }
}

fn parse_common(args: &[String]) -> Result<CommonArgs, String> {
    let mut parsed = CommonArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--root requires a path".to_string())?;
                parsed.roots.push(SearchRoot::new(value));
            }
            "--limit" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--limit requires a number".to_string())?;
                parsed.limit = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| "--limit must be a positive integer".to_string())?,
                );
            }
            "--json" => parsed.json = true,
            "--include-hidden" => parsed.include_hidden = true,
            "--include-ignored" => parsed.include_ignored = true,
            "--ext" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--ext requires an extension".to_string())?;
                parsed
                    .extensions
                    .push(value.trim_start_matches('.').to_ascii_lowercase());
            }
            "--query" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--query requires text".to_string())?;
                parsed.query_text = Some(value.clone());
            }
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
        index += 1;
    }
    Ok(parsed)
}

fn usage() -> String {
    "usage: kfs search <query> --root <path> [--limit n] [--json] | kfs explain <path> --root <path> [--query text] | kfs bench <query> --root <path>".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn search_requires_an_explicit_root() {
        let err = parse_args(&strings(&["search", "package json"])).unwrap_err();

        assert_eq!(err, "at least one --root is required");
    }

    #[test]
    fn parses_search_roots_limit_json_and_extensions() {
        let parsed = parse_args(&strings(&[
            "search",
            "package json",
            "--root",
            "/Users/alice/Dev",
            "--limit",
            "7",
            "--json",
            "--ext",
            "json",
        ]))
        .unwrap();

        assert_eq!(parsed.roots, vec![SearchRoot::new("/Users/alice/Dev")]);
        assert_eq!(
            parsed.command,
            CommandSpec::Search {
                query: SearchQuery {
                    query: "package json".to_string(),
                    limit: 7,
                    include_hidden: false,
                    include_ignored: false,
                    extensions: vec!["json".to_string()],
                },
                json: true,
            }
        );
    }

    #[test]
    fn parses_explain_with_optional_query() {
        let parsed = parse_args(&strings(&[
            "explain",
            "/Users/alice/Dev/project/package.json",
            "--root",
            "/Users/alice/Dev",
            "--query",
            "package json",
        ]))
        .unwrap();

        assert_eq!(
            parsed.command,
            CommandSpec::Explain {
                path: PathBuf::from("/Users/alice/Dev/project/package.json"),
                query: Some(SearchQuery::new("package json")),
            }
        );
    }

    #[test]
    fn parses_bench() {
        let parsed = parse_args(&strings(&[
            "bench",
            "main rs",
            "--root",
            "/Users/alice/Dev",
            "--limit",
            "3",
        ]))
        .unwrap();

        assert_eq!(
            parsed.command,
            CommandSpec::Bench {
                query: SearchQuery {
                    query: "main rs".to_string(),
                    limit: 3,
                    include_hidden: false,
                    include_ignored: false,
                    extensions: Vec::new(),
                },
            }
        );
    }
}
