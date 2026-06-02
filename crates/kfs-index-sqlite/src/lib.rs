//! SQLite-backed metadata index for the file search prototype.
//!
//! This crate owns a separate file-search database. It does not use the
//! desktop app database and can be rebuilt or queried by the CLI, daemon, or
//! future adapters.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kfs_core::{
    ranking::{score_candidate, tokenize},
    BackendError, EntryKind, IndexRebuildStats, IndexRootStatus, MatchKind, MetadataIndex,
    SearchCandidate, SearchConfig, SearchQuery, SearchResult, SearchRoot,
};
use kfs_crawler::{crawl, CrawlOptions};
use rusqlite::{params, Connection, OptionalExtension};

pub type RebuildStats = IndexRebuildStats;
pub type RootStatus = IndexRootStatus;

#[derive(Debug)]
pub struct SqliteIndex {
    conn: Connection,
}

impl MetadataIndex for SqliteIndex {
    fn rebuild_index(&mut self, config: &SearchConfig) -> Result<IndexRebuildStats, BackendError> {
        self.rebuild(config).map_err(sqlite_error)
    }

    fn search_index(
        &self,
        config: &SearchConfig,
        query: &SearchQuery,
    ) -> Result<Vec<SearchResult>, BackendError> {
        self.search(config, query).map_err(sqlite_error)
    }

    fn status(&self) -> Result<Vec<IndexRootStatus>, BackendError> {
        SqliteIndex::status(self).map_err(sqlite_error)
    }
}

impl SqliteIndex {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    pub fn open_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    pub fn rebuild(&mut self, config: &SearchConfig) -> rusqlite::Result<RebuildStats> {
        let now = unix_now();
        let mut total_entries = 0;
        let mut total_skipped = 0;
        let mut errors = Vec::new();

        let tx = self.conn.transaction()?;
        for root in config.roots.iter().filter(|root| root.enabled) {
            let root_id = upsert_root_tx(&tx, root)?;
            tx.execute("DELETE FROM entries WHERE root_id = ?1", params![root_id])?;

            let root_config = SearchConfig {
                roots: vec![root.clone()],
            };
            let options = CrawlOptions::new(root_config);
            let (entries, stats) = crawl(&options);
            total_skipped += stats.skipped;
            errors.extend(stats.errors);

            for entry in entries {
                let entry_id = insert_entry_tx(&tx, root_id, &entry, now)?;
                insert_terms_tx(&tx, entry_id, &entry.path)?;
                total_entries += 1;
            }

            tx.execute(
                "INSERT INTO root_state (root_id, generation, last_full_scan_at, last_incremental_at, dirty, entry_count)
                 VALUES (?1, COALESCE((SELECT generation + 1 FROM root_state WHERE root_id = ?1), 1), ?2, NULL, 0, ?3)
                 ON CONFLICT(root_id) DO UPDATE SET
                   generation = root_state.generation + 1,
                   last_full_scan_at = excluded.last_full_scan_at,
                   dirty = 0,
                   entry_count = excluded.entry_count",
                params![root_id, now, count_entries_tx(&tx, root_id)?],
            )?;
        }
        tx.commit()?;

        Ok(RebuildStats {
            roots: config.roots.iter().filter(|root| root.enabled).count(),
            entries: total_entries,
            skipped: total_skipped,
            errors,
        })
    }

    pub fn search(
        &self,
        config: &SearchConfig,
        query: &SearchQuery,
    ) -> rusqlite::Result<Vec<SearchResult>> {
        let mut entry_ids = matching_entry_ids(&self.conn, config, query)?;
        if entry_ids.is_empty() {
            return Ok(Vec::new());
        }
        entry_ids.sort_unstable();

        let mut results = Vec::new();
        for entry_id in entry_ids {
            if let Some(row) = load_entry(&self.conn, entry_id)? {
                let candidate = SearchCandidate {
                    path: row.path,
                    kind: row.kind,
                    provider: "sqlite".to_string(),
                };
                let (score, matches) = score_candidate(query, &candidate, row.root_priority);
                if score > 0 && extension_allowed(&candidate.path, query) {
                    results.push(SearchResult {
                        path: candidate.path,
                        score,
                        provider: candidate.provider,
                        matches,
                    });
                }
            }
        }

        results.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.path.cmp(&right.path))
        });
        results.truncate(query.limit);
        Ok(results)
    }

    pub fn status(&self) -> rusqlite::Result<Vec<RootStatus>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.path, COALESCE(s.entry_count, 0), COALESCE(s.generation, 0), COALESCE(s.dirty, 0),
                    s.last_full_scan_at, s.last_incremental_at
             FROM roots r
             LEFT JOIN root_state s ON s.root_id = r.id
             ORDER BY r.path",
        )?;
        let rows = stmt.query_map([], |row| {
            let entry_count: i64 = row.get(1)?;
            Ok(RootStatus {
                path: PathBuf::from(row.get::<_, String>(0)?),
                entry_count: usize::try_from(entry_count).unwrap_or(0),
                generation: row.get(2)?,
                dirty: row.get::<_, i64>(3)? != 0,
                last_full_scan_at: row.get(4)?,
                last_incremental_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS roots (
              id INTEGER PRIMARY KEY,
              path TEXT NOT NULL UNIQUE,
              enabled INTEGER NOT NULL,
              priority INTEGER NOT NULL DEFAULT 0,
              include_hidden INTEGER NOT NULL DEFAULT 0,
              include_ignored INTEGER NOT NULL DEFAULT 0,
              updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS entries (
              id INTEGER PRIMARY KEY,
              root_id INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              name TEXT NOT NULL,
              name_lower TEXT NOT NULL,
              extension TEXT,
              kind INTEGER NOT NULL,
              size INTEGER,
              mtime INTEGER,
              hidden INTEGER NOT NULL DEFAULT 0,
              ignored INTEGER NOT NULL DEFAULT 0,
              sensitive INTEGER NOT NULL DEFAULT 0,
              deleted INTEGER NOT NULL DEFAULT 0,
              indexed_at INTEGER NOT NULL,
              UNIQUE(root_id, path)
            );

            CREATE TABLE IF NOT EXISTS terms (
              term TEXT NOT NULL,
              entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
              field INTEGER NOT NULL,
              weight INTEGER NOT NULL,
              PRIMARY KEY(term, entry_id, field)
            );

            CREATE TABLE IF NOT EXISTS root_state (
              root_id INTEGER PRIMARY KEY REFERENCES roots(id) ON DELETE CASCADE,
              generation INTEGER NOT NULL,
              last_full_scan_at INTEGER,
              last_incremental_at INTEGER,
              dirty INTEGER NOT NULL DEFAULT 0,
              entry_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_entries_root_deleted ON entries(root_id, deleted);
            CREATE INDEX IF NOT EXISTS idx_entries_path ON entries(path);
            CREATE INDEX IF NOT EXISTS idx_entries_ext ON entries(extension);
            CREATE INDEX IF NOT EXISTS idx_terms_term ON terms(term);
            CREATE INDEX IF NOT EXISTS idx_terms_entry ON terms(entry_id);
            ",
        )
    }
}

#[derive(Debug)]
struct EntryRow {
    path: PathBuf,
    kind: EntryKind,
    root_priority: i32,
}

fn upsert_root_tx(tx: &rusqlite::Transaction<'_>, root: &SearchRoot) -> rusqlite::Result<i64> {
    let now = unix_now();
    tx.execute(
        "INSERT INTO roots (path, enabled, priority, include_hidden, include_ignored, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
           enabled = excluded.enabled,
           priority = excluded.priority,
           include_hidden = excluded.include_hidden,
           include_ignored = excluded.include_ignored,
           updated_at = excluded.updated_at",
        params![
            root.path.to_string_lossy(),
            root.enabled,
            root.priority,
            root.include_hidden,
            root.include_ignored,
            now
        ],
    )?;
    tx.query_row(
        "SELECT id FROM roots WHERE path = ?1",
        params![root.path.to_string_lossy()],
        |row| row.get(0),
    )
}

fn insert_entry_tx(
    tx: &rusqlite::Transaction<'_>,
    root_id: i64,
    entry: &kfs_crawler::CrawledEntry,
    indexed_at: i64,
) -> rusqlite::Result<i64> {
    let name = entry
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let extension = entry
        .path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    tx.execute(
        "INSERT INTO entries
         (root_id, path, name, name_lower, extension, kind, size, mtime, hidden, ignored, sensitive, deleted, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12)",
        params![
            root_id,
            entry.path.to_string_lossy(),
            name,
            name.to_ascii_lowercase(),
            extension,
            kind_to_int(entry.kind),
            entry.size.and_then(|size| i64::try_from(size).ok()),
            entry.mtime,
            entry.decision.hidden,
            entry.decision.ignored,
            entry.decision.sensitive,
            indexed_at
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

fn insert_terms_tx(
    tx: &rusqlite::Transaction<'_>,
    entry_id: i64,
    path: &Path,
) -> rusqlite::Result<()> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    for term in tokenize(name) {
        tx.execute(
            "INSERT OR IGNORE INTO terms (term, entry_id, field, weight) VALUES (?1, ?2, 1, 100)",
            params![term, entry_id],
        )?;
    }
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        tx.execute(
            "INSERT OR IGNORE INTO terms (term, entry_id, field, weight) VALUES (?1, ?2, 2, 50)",
            params![extension.to_ascii_lowercase(), entry_id],
        )?;
    }
    for component in path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
    {
        for term in tokenize(component) {
            tx.execute(
                "INSERT OR IGNORE INTO terms (term, entry_id, field, weight) VALUES (?1, ?2, 3, 10)",
                params![term, entry_id],
            )?;
        }
    }
    Ok(())
}

fn count_entries_tx(tx: &rusqlite::Transaction<'_>, root_id: i64) -> rusqlite::Result<i64> {
    tx.query_row(
        "SELECT COUNT(*) FROM entries WHERE root_id = ?1 AND deleted = 0",
        params![root_id],
        |row| row.get(0),
    )
}

fn matching_entry_ids(
    conn: &Connection,
    config: &SearchConfig,
    query: &SearchQuery,
) -> rusqlite::Result<Vec<i64>> {
    let root_ids = root_ids_for_config(conn, config)?;
    if root_ids.is_empty() {
        return Ok(Vec::new());
    }

    let query_terms = tokenize(&query.query);
    let mut intersection: Option<HashSet<i64>> = None;
    for term in query_terms {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT e.id
             FROM terms t
             JOIN entries e ON e.id = t.entry_id
             WHERE t.term LIKE ?1 AND e.deleted = 0",
        )?;
        let ids = stmt
            .query_map(params![format!("{term}%")], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        intersection = Some(match intersection {
            Some(existing) => existing.intersection(&ids).copied().collect(),
            None => ids,
        });
    }

    let Some(ids) = intersection else {
        let mut stmt = conn.prepare("SELECT id FROM entries WHERE deleted = 0")?;
        return stmt
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>();
    };

    let root_id_set = root_ids.into_iter().collect::<HashSet<_>>();
    let mut filtered = Vec::new();
    for id in ids {
        let root_id = conn
            .query_row(
                "SELECT root_id FROM entries WHERE id = ?1",
                params![id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if root_id.is_some_and(|root_id| root_id_set.contains(&root_id)) {
            filtered.push(id);
        }
    }
    Ok(filtered)
}

fn root_ids_for_config(conn: &Connection, config: &SearchConfig) -> rusqlite::Result<Vec<i64>> {
    let mut ids = Vec::new();
    for root in config.roots.iter().filter(|root| root.enabled) {
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM roots WHERE path = ?1",
                params![root.path.to_string_lossy()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn load_entry(conn: &Connection, entry_id: i64) -> rusqlite::Result<Option<EntryRow>> {
    conn.query_row(
        "SELECT e.path, e.kind, r.priority
         FROM entries e
         JOIN roots r ON r.id = e.root_id
         WHERE e.id = ?1 AND e.deleted = 0",
        params![entry_id],
        |row| {
            Ok(EntryRow {
                path: PathBuf::from(row.get::<_, String>(0)?),
                kind: int_to_kind(row.get(1)?),
                root_priority: row.get(2)?,
            })
        },
    )
    .optional()
}

fn extension_allowed(path: &Path, query: &SearchQuery) -> bool {
    query.extensions.is_empty()
        || path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                query.extensions.iter().any(|allowed| {
                    allowed
                        .trim_start_matches('.')
                        .eq_ignore_ascii_case(extension)
                })
            })
}

fn kind_to_int(kind: EntryKind) -> i64 {
    match kind {
        EntryKind::File => 1,
        EntryKind::Directory => 2,
        EntryKind::Other => 3,
    }
}

fn int_to_kind(value: i64) -> EntryKind {
    match value {
        1 => EntryKind::File,
        2 => EntryKind::Directory,
        _ => EntryKind::Other,
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

fn sqlite_error(error: rusqlite::Error) -> BackendError {
    BackendError::new(error.to_string())
}

pub fn match_names(matches: &[MatchKind]) -> Vec<&'static str> {
    matches
        .iter()
        .map(|kind| match kind {
            MatchKind::Empty => "Empty",
            MatchKind::ExactBasename => "ExactBasename",
            MatchKind::BasenamePrefix => "BasenamePrefix",
            MatchKind::BasenameToken => "BasenameToken",
            MatchKind::PathComponent => "PathComponent",
            MatchKind::Substring => "Substring",
            MatchKind::Extension => "Extension",
            MatchKind::RootPriority => "RootPriority",
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use kfs_crawler::remove_dir_all_if_exists;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = unix_now();
        std::env::temp_dir().join(format!("kfs-index-{name}-{nonce}-{}", std::process::id()))
    }

    #[test]
    fn rebuild_indexes_allowed_files_and_searches_terms() {
        let root = temp_dir("rebuild");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("docs/auth-providers.md"), "auth\n").unwrap();
        fs::write(root.join("node_modules/pkg/auth-providers.md"), "ignored\n").unwrap();

        let mut index = SqliteIndex::open_memory().unwrap();
        let config = SearchConfig {
            roots: vec![SearchRoot::new(&root)],
        };
        let stats = index.rebuild(&config).unwrap();
        let results = index
            .search(&config, &SearchQuery::new("auth providers"))
            .unwrap();
        remove_dir_all_if_exists(&root).unwrap();

        assert!(stats.entries >= 2);
        assert!(stats.skipped >= 1);
        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("docs/auth-providers.md"));
        assert_eq!(results[0].provider, "sqlite");
    }

    #[test]
    fn status_reports_rebuilt_root() {
        let root = temp_dir("status");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();

        let mut index = SqliteIndex::open_memory().unwrap();
        let config = SearchConfig {
            roots: vec![SearchRoot::new(&root)],
        };
        let expected_root = config.roots[0].path.clone();
        index.rebuild(&config).unwrap();
        let status = index.status().unwrap();
        remove_dir_all_if_exists(&root).unwrap();

        assert_eq!(status.len(), 1);
        assert_eq!(status[0].path, expected_root);
        assert!(status[0].entry_count >= 1);
        assert!(!status[0].dirty);
    }
}
