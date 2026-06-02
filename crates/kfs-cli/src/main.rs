//! Command-line adapter for the independent Rust file search prototype.
//!
//! The CLI exists for local testing and future integration experiments. It
//! requires explicit roots and delegates filtering/ranking to `kfs-core`.

mod args;
mod output;

use std::process::ExitCode;
use std::time::Instant;

use args::{parse_args, CommandSpec};
use kfs_core::{SearchConfig, SearchEngineCore};
use kfs_provider_spotlight::SpotlightProvider;
use output::{format_explain_text, format_results_json, format_results_text};

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("kfs: {err}");
            ExitCode::from(2)
        }
    }
}

fn run(raw_args: Vec<String>) -> Result<String, String> {
    let spec = parse_args(&raw_args)?;
    let config = SearchConfig { roots: spec.roots };
    let core = SearchEngineCore::new(config.clone());

    match spec.command {
        CommandSpec::Search { query, json } => {
            let provider = SpotlightProvider::default();
            let candidates = provider
                .search(&config, &query)
                .map_err(|err| err.to_string())?;
            let results = core.search_candidates(&query, candidates);
            if json {
                Ok(format_results_json(&results))
            } else {
                Ok(format_results_text(&results))
            }
        }
        CommandSpec::Explain { path, query } => {
            let explain = core.explain_path(&path, query.as_ref());
            Ok(format_explain_text(&explain))
        }
        CommandSpec::Bench { query } => {
            let started = Instant::now();
            let provider = SpotlightProvider::default();
            let candidates = provider
                .search(&config, &query)
                .map_err(|err| err.to_string())?;
            let results = core.search_candidates(&query, candidates);
            Ok(format!(
                "results={} elapsed_ms={}",
                results.len(),
                started.elapsed().as_millis()
            ))
        }
    }
}
