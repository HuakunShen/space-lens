//! Read-only audit of per-application storage shared by Space Lens shells.
//!
//! A new Rust crate combining two layers: an auto layer that enumerates
//! installed apps, parses bundle ids from `Contents/Info.plist`, and measures
//! conventional per-bundle-id locations with kuntu's `measure_path`; and a
//! catalog layer (a per-platform JSON file embedded in the binary) that adds
//! known non-conventional locations, classifications, and comments.
//!
//! The audit never deletes, moves, trashes, or writes anything. Sizes are
//! lower bounds where the OS denies reads (`measure_path` fails soft to 0),
//! and per-app totals never double-count: after merging heuristic and catalog
//! locations, any parent path is replaced by a residual entry sized
//! `parent - sum(children)`.

#![deny(clippy::all)]

mod catalog;
mod macos;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use catalog::{macos_catalog, Catalog, CatalogEntry, CatalogLocation, CatalogMatch};

#[derive(Clone, Debug)]
pub struct AppAuditOptions {
  /// Locations smaller than this many bytes are dropped.
  pub min_size: u64,
  /// Include apps that only have heuristic (non-catalog) locations.
  pub include_heuristic: bool,
  /// Max apps returned after the size-descending sort. Default 500.
  pub limit: usize,
  /// Home directory override (tests). `None` reads env HOME.
  pub home: Option<PathBuf>,
}

/// One enumerated application before measurement.
#[derive(Clone, Debug)]
pub struct DiscoveredApp {
  pub name: String,
  pub bundle_id: Option<String>,
  pub app_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
  App,
  Cli,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditSource {
  Catalog,
  Heuristic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
  Application,
  Cache,
  Versions,
  Logs,
  #[serde(rename = "re-downloadable")]
  Redownloadable,
  UserData,
  Support,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAuditReport {
  pub platform: String,
  pub generated_at: String,
  pub apps: Vec<AppAuditEntry>,
  /// App count after filtering, before `limit` truncation.
  pub total_apps: usize,
  pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAuditEntry {
  /// Catalog id, else bundle id, else a kebab-case slug of the display name.
  pub id: String,
  pub name: String,
  pub kind: AppKind,
  pub source: AuditSource,
  pub bundle_id: Option<String>,
  /// The `.app` bundle path; `None` for CLI entries.
  pub app_path: Option<String>,
  /// The `.app` bundle size; 0 for CLI entries.
  pub installed_size: u64,
  /// `installed_size` plus all reported locations.
  pub total_size: u64,
  /// Size-descending locations.
  pub locations: Vec<AppAuditLocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppAuditLocation {
  /// Canonical absolute path (`~` already expanded).
  pub path: String,
  pub label: Option<String>,
  pub classification: Classification,
  pub purgeable: bool,
  pub comment: Option<String>,
  pub size: u64,
  pub source: AuditSource,
}

#[derive(Debug)]
pub struct AppAuditError(pub String);

impl fmt::Display for AppAuditError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for AppAuditError {}

/// Enumerate installed apps for the current user and audit their storage.
pub fn audit_installed_apps(options: &AppAuditOptions) -> Result<AppAuditReport, AppAuditError> {
  let home = match &options.home {
    Some(home) => home.clone(),
    None => std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
      AppAuditError("HOME is not set; pass AppAuditOptions.home instead".to_string())
    })?,
  };
  let apps = macos::enumerate_installed_apps(&home);
  audit_with(&apps, &home, options)
}

/// Testability seam: audit an explicit app list against an explicit home.
pub fn audit_with(
  apps: &[DiscoveredApp],
  home: &Path,
  options: &AppAuditOptions,
) -> Result<AppAuditReport, AppAuditError> {
  let catalog = catalog::load_catalog().map_err(AppAuditError)?;
  // Canonicalize once so `~` expansion and all later prefix comparisons run
  // on canonical paths even when home itself sits under a symlink (macOS
  // temp dirs, /var -> /private/var).
  let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());

  // Dedup discovered apps by bundle id, keeping the largest bundle.
  let mut scored: Vec<ScoredApp> = Vec::with_capacity(apps.len());
  let mut by_bundle_id: HashMap<&str, usize> = HashMap::new();
  for app in apps {
    let installed_size = space_lens::measure_path(&app.app_path);
    let record = ScoredApp {
      name: app.name.clone(),
      bundle_id: app.bundle_id.clone(),
      app_path: app.app_path.clone(),
      installed_size,
    };
    match &app.bundle_id {
      Some(bundle_id) => match by_bundle_id.get(bundle_id.as_str()) {
        Some(&index) => {
          if installed_size > scored[index].installed_size {
            scored[index] = record;
          }
        }
        None => {
          by_bundle_id.insert(bundle_id.as_str(), scored.len());
          scored.push(record);
        }
      },
      None => scored.push(record),
    }
  }

  let mut entries: Vec<AppAuditEntry> = Vec::new();
  let mut used_ids: HashSet<String> = HashSet::new();

  for app in &scored {
    let matched: Vec<&CatalogEntry> = catalog
      .entries
      .iter()
      .filter(|entry| {
        entry.kind == AppKind::App && catalog_matches(entry, app.bundle_id.as_deref(), &app.name)
      })
      .collect();
    let mut table = LocationTable::default();
    table.add_heuristic(&home, &app.name, app.bundle_id.as_deref());
    for entry in &matched {
      table.add_catalog(&home, &entry.locations);
    }
    let locations = table.finish(options.min_size);
    let total_size = app.installed_size + locations.iter().map(|item| item.size).sum::<u64>();
    if locations.is_empty() && app.installed_size == 0 {
      continue;
    }
    // Display name and bundle id come from the discovered app; entry id and
    // name come from the catalog when matched.
    let (base_id, name, source) = match matched.first() {
      Some(entry) => (entry.id.clone(), entry.name.clone(), AuditSource::Catalog),
      None => match &app.bundle_id {
        Some(bundle_id) => (bundle_id.clone(), app.name.clone(), AuditSource::Heuristic),
        None => (slugify(&app.name), app.name.clone(), AuditSource::Heuristic),
      },
    };
    entries.push(AppAuditEntry {
      id: unique_id(&base_id, &mut used_ids),
      name,
      kind: AppKind::App,
      source,
      bundle_id: app.bundle_id.clone(),
      app_path: Some(app.app_path.to_string_lossy().into_owned()),
      installed_size: app.installed_size,
      total_size,
      locations,
    });
  }

  // CLI catalog entries stand alone: no app bundle, `installed_size` 0.
  for entry in catalog
    .entries
    .iter()
    .filter(|entry| entry.kind == AppKind::Cli)
  {
    let mut table = LocationTable::default();
    table.add_catalog(&home, &entry.locations);
    let locations = table.finish(options.min_size);
    let total_size = locations.iter().map(|item| item.size).sum::<u64>();
    if locations.is_empty() {
      continue;
    }
    entries.push(AppAuditEntry {
      id: unique_id(&entry.id, &mut used_ids),
      name: entry.name.clone(),
      kind: AppKind::Cli,
      source: AuditSource::Catalog,
      bundle_id: None,
      app_path: None,
      installed_size: 0,
      total_size,
      locations,
    });
  }

  if !options.include_heuristic {
    entries.retain(|entry| entry.source == AuditSource::Catalog);
  }
  entries.sort_by(|a, b| {
    b.total_size
      .cmp(&a.total_size)
      .then_with(|| a.name.cmp(&b.name))
  });
  let total_apps = entries.len();
  let truncated = total_apps > options.limit;
  entries.truncate(options.limit);

  Ok(AppAuditReport {
    platform: std::env::consts::OS.to_string(),
    generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    apps: entries,
    total_apps,
    truncated,
  })
}

struct ScoredApp {
  name: String,
  bundle_id: Option<String>,
  app_path: PathBuf,
  installed_size: u64,
}

fn catalog_matches(entry: &CatalogEntry, bundle_id: Option<&str>, display_name: &str) -> bool {
  let Some(matched) = &entry.r#match else {
    return false;
  };
  if let Some(bundle_id) = bundle_id {
    if matched
      .bundle_ids
      .iter()
      .any(|candidate| candidate == bundle_id)
    {
      return true;
    }
  }
  matched
    .app_names
    .iter()
    .any(|candidate| candidate.eq_ignore_ascii_case(display_name))
}

#[derive(Clone, Debug)]
struct LocationMeta {
  label: Option<String>,
  classification: Classification,
  purgeable: bool,
  comment: Option<String>,
  source: AuditSource,
}

#[derive(Default)]
struct LocationTable {
  inner: HashMap<PathBuf, LocationMeta>,
}

impl LocationTable {
  fn add_heuristic(&mut self, home: &Path, name: &str, bundle_id: Option<&str>) {
    for (relative, classification) in conventional_candidates(name, bundle_id) {
      let raw = home.join(relative);
      let Some(canonical) = existing_canonical(&raw) else {
        continue;
      };
      self.inner.entry(canonical).or_insert(LocationMeta {
        label: None,
        classification,
        purgeable: default_purgeable(classification),
        comment: None,
        source: AuditSource::Heuristic,
      });
    }
  }

  fn add_catalog(&mut self, home: &Path, locations: &[CatalogLocation]) {
    for location in locations {
      let raw = expand_tilde(&location.path, home);
      let Some(canonical) = existing_canonical(&raw) else {
        continue;
      };
      // Catalog metadata wins when a resolved path collides exactly.
      self.inner.insert(
        canonical,
        LocationMeta {
          label: location.label.clone(),
          classification: location.classification,
          purgeable: location.purgeable,
          comment: location.comment.clone(),
          source: AuditSource::Catalog,
        },
      );
    }
  }

  /// Merge nested paths via the generalized residual rule: children stay, and
  /// any parent is replaced by a residual sized `parent - sum(children)`
  /// (clamped at 0, and a zero residual disappears). Residuals go through the
  /// same `min_size` filter as every other location.
  fn finish(self, min_size: u64) -> Vec<AppAuditLocation> {
    let mut measured: HashMap<&Path, u64> = HashMap::with_capacity(self.inner.len());
    for path in self.inner.keys() {
      measured.insert(path.as_path(), space_lens::measure_path(path));
    }
    let collected: HashSet<&Path> = self.inner.keys().map(PathBuf::as_path).collect();
    let mut child_sums: HashMap<&Path, u64> = HashMap::new();
    let mut has_children: HashSet<&Path> = HashSet::new();
    for path in self.inner.keys() {
      let path = path.as_path();
      for ancestor in path.ancestors().skip(1) {
        if collected.contains(ancestor) {
          *child_sums.entry(ancestor).or_default() += measured[path];
          has_children.insert(ancestor);
          break;
        }
      }
    }
    let mut locations = Vec::with_capacity(self.inner.len());
    for (path, meta) in &self.inner {
      let size = measured[path.as_path()];
      if has_children.contains(path.as_path()) {
        let residual = size.saturating_sub(child_sums[path.as_path()]);
        if residual == 0 || residual < min_size {
          continue;
        }
        locations.push(AppAuditLocation {
          path: path.to_string_lossy().into_owned(),
          label: Some("Other data".to_string()),
          classification: Classification::Support,
          purgeable: false,
          comment: None,
          size: residual,
          source: meta.source,
        });
      } else {
        if size < min_size {
          continue;
        }
        locations.push(AppAuditLocation {
          path: path.to_string_lossy().into_owned(),
          label: meta.label.clone(),
          classification: meta.classification,
          purgeable: meta.purgeable,
          comment: meta.comment.clone(),
          size,
          source: meta.source,
        });
      }
    }
    locations.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    locations
  }
}

/// Conventional per-bundle-id locations from the plan's table. Apps without a
/// bundle id fall back to name-only candidates.
fn conventional_candidates(name: &str, bundle_id: Option<&str>) -> Vec<(PathBuf, Classification)> {
  use Classification::{Cache, Logs, Support};
  let primary = bundle_id.unwrap_or(name);
  let mut candidates = Vec::new();
  for key in [primary, name] {
    candidates.push((format!("Library/Application Support/{key}"), Support));
    candidates.push((format!("Library/Caches/{key}"), Cache));
    candidates.push((format!("Library/Logs/{key}"), Logs));
  }
  candidates.push((format!("Library/WebKit/{primary}"), Cache));
  candidates.push((format!("Library/HTTPStorages/{primary}"), Cache));
  candidates.push((
    format!("Library/Saved Application State/{primary}.savedState"),
    Cache,
  ));
  candidates.push((format!("Library/Cookies/{primary}.binarycookies"), Cache));
  candidates.push((format!("Library/Containers/{primary}"), Support));
  candidates.push((format!("Library/Application Scripts/{primary}"), Support));
  candidates.push((format!("Library/Preferences/{primary}.plist"), Support));
  candidates
    .into_iter()
    .map(|(path, classification)| (PathBuf::from(path), classification))
    .collect()
}

fn default_purgeable(classification: Classification) -> bool {
  match classification {
    Classification::Application | Classification::Support => false,
    Classification::Cache
    | Classification::Versions
    | Classification::Logs
    | Classification::Redownloadable
    | Classification::UserData => true,
  }
}

/// Resolve an existing non-symlink path to its canonical absolute form.
/// Missing paths and symlinks are skipped (measurement never follows links).
fn existing_canonical(path: &Path) -> Option<PathBuf> {
  match std::fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_symlink() => None,
    Ok(_) => std::fs::canonicalize(path).ok(),
    Err(_) => None,
  }
}

fn expand_tilde(path: &str, home: &Path) -> PathBuf {
  match path.strip_prefix("~/") {
    Some(rest) => home.join(rest),
    None => PathBuf::from(path),
  }
}

fn slugify(name: &str) -> String {
  let mut slug = String::new();
  for part in name
    .to_lowercase()
    .split(|character: char| !character.is_ascii_alphanumeric())
    .filter(|part| !part.is_empty())
  {
    if !slug.is_empty() {
      slug.push('-');
    }
    slug.push_str(part);
  }
  if slug.is_empty() {
    slug.push_str("app");
  }
  slug
}

fn unique_id(base: &str, used: &mut HashSet<String>) -> String {
  if used.insert(base.to_string()) {
    return base.to_string();
  }
  let mut counter = 2;
  loop {
    let candidate = format!("{base}-{counter}");
    if used.insert(candidate.clone()) {
      return candidate;
    }
    counter += 1;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;

  struct Fixture {
    root: PathBuf,
  }

  impl Fixture {
    fn new(tag: &str) -> Fixture {
      let root =
        std::env::temp_dir().join(format!("spacelens-app-audit-{tag}-{}", std::process::id()));
      let _ = std::fs::remove_dir_all(&root);
      std::fs::create_dir_all(&root).unwrap();
      // Canonicalize up front so expected join paths match the canonical
      // absolute paths the audit reports.
      let root = root.canonicalize().unwrap();
      Fixture { root }
    }

    fn home(&self) -> PathBuf {
      let home = self.root.join("home");
      std::fs::create_dir_all(&home).unwrap();
      home
    }

    fn file(&self, relative: &str, bytes: usize) -> PathBuf {
      let path = self.root.join("home").join(relative);
      if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      let mut file = std::fs::File::create(&path).unwrap();
      // Pseudo-random (xorshift) content: incompressible, so allocated sizes
      // track the requested byte counts on compressing filesystems.
      let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
      let mut remaining = bytes;
      let mut chunk = [0u8; 8192];
      while remaining > 0 {
        for byte in chunk.iter_mut() {
          state ^= state << 13;
          state ^= state >> 7;
          state ^= state << 17;
          *byte = (state >> 32) as u8;
        }
        let take = remaining.min(chunk.len());
        file.write_all(&chunk[..take]).unwrap();
        remaining -= take;
      }
      path
    }
  }

  impl Drop for Fixture {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.root);
    }
  }

  fn options(min_size: u64, include_heuristic: bool, limit: usize) -> AppAuditOptions {
    AppAuditOptions {
      min_size,
      include_heuristic,
      limit,
      home: None,
    }
  }

  fn discovered(name: &str, bundle_id: Option<&str>, home: &Path) -> DiscoveredApp {
    DiscoveredApp {
      name: name.to_string(),
      bundle_id: bundle_id.map(str::to_string),
      app_path: home.join(format!("Applications/{name}.app")),
    }
  }

  fn location_paths(entry: &AppAuditEntry) -> Vec<String> {
    entry
      .locations
      .iter()
      .map(|item| item.path.clone())
      .collect()
  }

  #[test]
  fn catalog_validates() {
    let catalog = macos_catalog();
    assert_eq!(catalog.schema_version, 1);
    assert_eq!(catalog.platform, "macos");
    assert_eq!(catalog.entries.len(), 15);
    // The audit rejects the same invalid shapes it asserts here.
    crate::catalog::load_catalog().expect("embedded catalog loads");
    let mut ids = HashSet::new();
    for entry in &catalog.entries {
      assert!(ids.insert(entry.id.clone()), "duplicate id: {}", entry.id);
      assert!(!entry.locations.is_empty(), "no locations: {}", entry.id);
      if entry.kind == AppKind::App {
        assert!(
          entry.r#match.is_some(),
          "app-kind entry has null match: {}",
          entry.id
        );
      }
      for location in &entry.locations {
        assert!(
          location.path.starts_with("~/"),
          "non-home path: {}",
          location.path
        );
      }
    }
  }

  #[test]
  fn conventional_candidates_expand() {
    let fixture = Fixture::new("candidates");
    let home = fixture.home();
    let bundle_id = "com.example.TestApp";
    fixture.file(
      "Library/Application Support/com.example.TestApp/data.bin",
      100,
    );
    fixture.file("Library/Caches/TestApp/cache.bin", 100);
    fixture.file("Library/Logs/com.example.TestApp/log.txt", 100);
    fixture.file("Library/Containers/com.example.TestApp/Settings.plist", 10);

    let apps = vec![discovered("TestApp", Some(bundle_id), &home)];
    let report = audit_with(&apps, &home, &options(0, true, 500)).unwrap();
    assert_eq!(report.apps.len(), 1);
    let entry = &report.apps[0];
    assert_eq!(entry.source, AuditSource::Heuristic);
    assert_eq!(entry.bundle_id.as_deref(), Some(bundle_id));
    let paths = location_paths(entry);
    let expect = |relative: &str| home.join(relative).to_string_lossy().into_owned();
    for relative in [
      "Library/Application Support/com.example.TestApp",
      "Library/Caches/TestApp",
      "Library/Logs/com.example.TestApp",
      "Library/Containers/com.example.TestApp",
    ] {
      assert!(
        paths.contains(&expect(relative)),
        "missing heuristic location: {relative}"
      );
    }
    // Candidates that were never created stay absent.
    assert!(!paths.iter().any(|path| path.contains("WebKit")));
    assert!(!paths.iter().any(|path| path.contains("HTTPStorages")));
    assert!(!paths
      .iter()
      .any(|path| path.contains("Saved Application State")));
    let support = entry
      .locations
      .iter()
      .find(|item| item.path == expect("Library/Application Support/com.example.TestApp"))
      .unwrap();
    assert_eq!(support.classification, Classification::Support);
    assert!(!support.purgeable);
    assert_eq!(support.source, AuditSource::Heuristic);
    let caches = entry
      .locations
      .iter()
      .find(|item| item.path == expect("Library/Caches/TestApp"))
      .unwrap();
    assert_eq!(caches.classification, Classification::Cache);
    assert!(caches.purgeable);
  }

  #[test]
  fn residual_rule_splits_parents() {
    let fixture = Fixture::new("residual");
    let home = fixture.home();
    fixture.file(".codex/sessions/session-1.jsonl", 4096);
    fixture.file(".codex/stray.bin", 2048);

    let report = audit_with(&[], &home, &options(0, true, 500)).unwrap();
    let entry = report
      .apps
      .iter()
      .find(|app| app.id == "codex-cli")
      .expect("codex-cli entry");
    let sessions = home.join(".codex/sessions").to_string_lossy().into_owned();
    let root = home.join(".codex").to_string_lossy().into_owned();
    let child = entry
      .locations
      .iter()
      .find(|item| item.path == sessions)
      .expect("sessions child");
    let residual = entry
      .locations
      .iter()
      .find(|item| item.path == root)
      .expect("residual root");
    assert_eq!(residual.label.as_deref(), Some("Other data"));
    assert_eq!(residual.classification, Classification::Support);
    assert!(!residual.purgeable);
    assert!(residual.comment.is_none());
    let parent_size = space_lens::measure_path(&home.join(".codex"));
    let child_size = space_lens::measure_path(&home.join(".codex/sessions"));
    assert_eq!(child.size, child_size);
    assert_eq!(residual.size, parent_size.saturating_sub(child_size));
    // No double count: the entry total equals the measured parent total.
    assert_eq!(entry.total_size, parent_size);
  }

  #[test]
  fn catalog_metadata_wins() {
    let fixture = Fixture::new("metadata");
    let home = fixture.home();
    fixture.file("Library/Application Support/com.raycast.macos/data.db", 100);

    let apps = vec![discovered("Raycast", Some("com.raycast.macos"), &home)];
    let report = audit_with(&apps, &home, &options(0, true, 500)).unwrap();
    let entry = report
      .apps
      .iter()
      .find(|app| app.id == "raycast")
      .expect("raycast entry");
    assert_eq!(entry.name, "Raycast");
    assert_eq!(entry.source, AuditSource::Catalog);
    let root = home
      .join("Library/Application Support/com.raycast.macos")
      .to_string_lossy()
      .into_owned();
    let location = entry
      .locations
      .iter()
      .find(|item| item.path == root)
      .expect("raycast data root");
    // The heuristic candidate for the same path carries no metadata; the
    // catalog label/classification/comment must win.
    assert_eq!(location.source, AuditSource::Catalog);
    assert_eq!(location.label.as_deref(), Some("Raycast data root"));
    assert_eq!(location.classification, Classification::Support);
    assert!(!location.purgeable);
    assert!(location.comment.is_some());
  }

  #[test]
  fn min_size_limit_truncated() {
    let fixture = Fixture::new("limits");
    let home = fixture.home();
    // Sizes straddle min_size with block-allocation margin: data files are
    // ~1 MiB allocated, the small log is a single block, and residuals are
    // bare directory metadata.
    fixture.file(".codex/sessions/a.jsonl", 1_048_576);
    fixture.file(".ollama/models/b.bin", 1_048_576);
    fixture.file(".npm/_logs/tiny.log", 1024);

    let report = audit_with(&[], &home, &options(262_144, true, 1)).unwrap();
    // npm-cache only has a single-block location: dropped by min_size, and
    // with no installed size the whole entry disappears.
    assert!(!report.apps.iter().any(|app| app.id == "npm-cache"));
    assert_eq!(report.total_apps, 2);
    assert!(report.truncated);
    assert_eq!(report.apps.len(), 1);
    for app in &report.apps {
      for location in &app.locations {
        assert!(
          location.size >= 262_144,
          "location below min_size survived: {}",
          location.path
        );
      }
    }
  }

  #[test]
  fn cli_entries_without_apps() {
    let fixture = Fixture::new("cli");
    let home = fixture.home();
    fixture.file(".codex/sessions/a.jsonl", 100);

    let report = audit_with(&[], &home, &options(0, true, 500)).unwrap();
    let entry = report
      .apps
      .iter()
      .find(|app| app.id == "codex-cli")
      .expect("cli entry without discovered apps");
    assert_eq!(entry.kind, AppKind::Cli);
    assert_eq!(entry.source, AuditSource::Catalog);
    assert!(entry.bundle_id.is_none());
    assert!(entry.app_path.is_none());
    assert_eq!(entry.installed_size, 0);
    assert!(!entry.locations.is_empty());
  }

  #[test]
  fn empty_home_empty_report() {
    let fixture = Fixture::new("empty");
    let home = fixture.home();
    let report = audit_with(&[], &home, &options(0, true, 500)).unwrap();
    assert!(report.apps.is_empty());
    assert_eq!(report.total_apps, 0);
    assert!(!report.truncated);
    assert_eq!(report.platform, std::env::consts::OS);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn bundle_id_from_plist() {
    let fixture = Fixture::new("plist");
    let bundle = fixture.root.join("Fake.app");
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(&contents).unwrap();
    std::fs::write(
      contents.join("Info.plist"),
      r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.example.FakeApp</string>
  <key>CFBundleName</key>
  <string>FakeApp</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    let (name, bundle_id) =
      crate::macos::read_bundle_identity(&bundle, "Fallback").expect("identity");
    assert_eq!(name, "FakeApp");
    assert_eq!(bundle_id.as_deref(), Some("com.example.FakeApp"));

    // Keys missing: fall back to the enumerated name with no bundle id.
    let partial = fixture.root.join("Partial.app");
    std::fs::create_dir_all(partial.join("Contents")).unwrap();
    std::fs::write(
      partial.join("Contents/Info.plist"),
      r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleVersion</key>
  <string>1</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    let (name, bundle_id) =
      crate::macos::read_bundle_identity(&partial, "Fallback").expect("identity");
    assert_eq!(name, "Fallback");
    assert!(bundle_id.is_none());

    // No Info.plist at all: the app is skipped.
    assert!(
      crate::macos::read_bundle_identity(&fixture.root.join("Missing.app"), "Fallback").is_none()
    );
  }

  #[test]
  fn report_serializes_with_camel_case_keys() {
    assert_eq!(
      serde_json::to_value(Classification::Redownloadable).unwrap(),
      serde_json::Value::String("re-downloadable".to_string())
    );
    assert_eq!(
      serde_json::to_value(Classification::UserData).unwrap(),
      serde_json::Value::String("user-data".to_string())
    );

    let fixture = Fixture::new("serde");
    let home = fixture.home();
    fixture.file(".codex/sessions/a.jsonl", 100);
    let apps = vec![discovered("Raycast", Some("com.raycast.macos"), &home)];
    let report = audit_with(&apps, &home, &options(0, true, 500)).unwrap();
    let text = serde_json::to_string(&serde_json::to_value(&report).unwrap()).unwrap();
    for key in [
      "generatedAt",
      "totalApps",
      "totalSize",
      "bundleId",
      "appPath",
      "installedSize",
    ] {
      assert!(text.contains(key), "missing camelCase key: {key}");
    }
    for key in [
      "generated_at",
      "total_apps",
      "total_size",
      "bundle_id",
      "app_path",
      "installed_size",
    ] {
      assert!(!text.contains(key), "snake_case key leaked: {key}");
    }
  }
}
