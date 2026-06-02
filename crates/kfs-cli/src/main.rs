//! Command-line adapter for the independent Rust file search prototype.
//!
//! The CLI exists for local testing and future integration experiments. It
//! requires explicit roots and delegates filtering/ranking to `kfs-core`.

mod args;
mod output;

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use args::{parse_args, CommandSpec, IndexCommand, SearchProvider};
use kfs_core::{
    CandidateProvider, MetadataIndex, SearchConfig, SearchEngineCore, SearchQuery, SearchResult,
};
use kfs_index_sqlite::SqliteIndex;
use kfs_provider_spotlight::SpotlightProvider;
use kfs_watcher::{run_polling_watch, WatchOptions};
use output::{
    format_explain_text, format_rebuild_stats, format_refresh_stats, format_repair_stats,
    format_results_json, format_results_text, format_status, format_watch_stats,
};

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
            let results = search_with_provider(
                &config,
                &core,
                &query,
                spec.provider,
                spec.db_path.as_deref(),
            )?;
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
            let results = search_with_provider(
                &config,
                &core,
                &query,
                spec.provider,
                spec.db_path.as_deref(),
            )?;
            Ok(format!(
                "results={} elapsed_ms={}",
                results.len(),
                started.elapsed().as_millis()
            ))
        }
        CommandSpec::Index(command) => run_index_command(&config, spec.db_path.as_deref(), command),
        CommandSpec::Watch { duration_ms } => {
            run_watch_command(&config, spec.db_path.as_deref(), duration_ms)
        }
    }
}

fn search_with_provider(
    config: &SearchConfig,
    core: &SearchEngineCore,
    query: &SearchQuery,
    provider: SearchProvider,
    db_path: Option<&Path>,
) -> Result<Vec<SearchResult>, String> {
    match provider {
        SearchProvider::Spotlight => search_spotlight(config, core, query),
        SearchProvider::Sqlite => {
            let path = db_path.ok_or_else(|| "--db is required for sqlite provider".to_string())?;
            search_sqlite(config, query, path)
        }
        SearchProvider::Auto => {
            if let Some(path) = db_path {
                let results = search_sqlite(config, query, path)?;
                if !results.is_empty() {
                    return Ok(results);
                }
            }
            search_spotlight(config, core, query)
        }
    }
}

fn search_spotlight(
    config: &SearchConfig,
    core: &SearchEngineCore,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, String> {
    let provider = SpotlightProvider::default();
    let candidates = provider
        .search_candidates(config, query)
        .map_err(|err| err.to_string())?;
    Ok(core.search_candidates(query, candidates))
}

fn search_sqlite(
    config: &SearchConfig,
    query: &SearchQuery,
    db_path: &Path,
) -> Result<Vec<SearchResult>, String> {
    let index = SqliteIndex::open(db_path).map_err(|err| err.to_string())?;
    index
        .search_index(config, query)
        .map_err(|err| err.to_string())
}

fn run_index_command(
    config: &SearchConfig,
    db_path: Option<&Path>,
    command: IndexCommand,
) -> Result<String, String> {
    let path = db_path.ok_or_else(|| "--db is required for index commands".to_string())?;
    let mut index = SqliteIndex::open(path).map_err(|err| err.to_string())?;
    match command {
        IndexCommand::Rebuild => {
            let stats = index.rebuild_index(config).map_err(|err| err.to_string())?;
            Ok(format_rebuild_stats(&stats))
        }
        IndexCommand::Refresh => {
            let stats = index.refresh_index(config).map_err(|err| err.to_string())?;
            Ok(format_refresh_stats(&stats))
        }
        IndexCommand::Repair => {
            let stats = index.repair_index(config).map_err(|err| err.to_string())?;
            Ok(format_repair_stats(&stats))
        }
        IndexCommand::Status => {
            let status = index.status().map_err(|err| err.to_string())?;
            Ok(format_status(&status))
        }
    }
}

fn run_watch_command(
    config: &SearchConfig,
    db_path: Option<&Path>,
    duration_ms: u64,
) -> Result<String, String> {
    let path = db_path.ok_or_else(|| "--db is required for watch".to_string())?;
    let mut index = SqliteIndex::open(path).map_err(|err| err.to_string())?;
    let options = WatchOptions::new(config.clone())
        .with_duration(Duration::from_millis(duration_ms))
        .with_poll_interval(Duration::from_millis(100));
    let stats = run_polling_watch(&options, |_event| index.refresh_index(config).map(|_| ()))
        .map_err(|err| err.to_string())?;
    if stats.dirty {
        for root in config.roots.iter().filter(|root| root.enabled) {
            index.mark_root_dirty(root).map_err(|err| err.to_string())?;
        }
    }
    Ok(format_watch_stats(&stats))
}
