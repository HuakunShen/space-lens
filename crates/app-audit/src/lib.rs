//! Read-only audit of per-application storage shared by Space Lens shells.
//!
//! A new Rust crate combining two layers: an auto layer that enumerates
//! installed apps, parses bundle ids from `Contents/Info.plist`, and measures
//! conventional per-bundle-id locations; and a catalog layer (a per-platform
//! JSON file embedded in the binary) that adds known non-conventional
//! locations, classifications, and comments.
//!
//! The audit never deletes, moves, trashes, or writes anything. Sizes are
//! lower bounds where the OS denies reads (failed reads measure as 0), and
//! per-app totals never double-count: after all entries' locations are
//! resolved and validated, every surviving location path plus every `.app`
//! bundle path is measured in ONE deepest-first `space_lens::scan_directory`
//! call whose shared seen-inode set partitions overlapping paths, so a parent
//! location keeps its label/classification but its size excludes measured
//! children. Hardlinked files are attributed exactly once.

#![deny(clippy::all)]

mod catalog;
mod macos;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use space_lens::{scan_directory, IgnoredMode, ScanOptions};

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

/// One report entry before measurement: resolved locations plus identity.
struct PendingEntry {
  base_id: String,
  name: String,
  kind: AppKind,
  source: AuditSource,
  bundle_id: Option<String>,
  app_path: Option<PathBuf>,
  app_canonical: Option<PathBuf>,
  locations: HashMap<PathBuf, LocationMeta>,
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

  // Resolve every entry's locations (existence + path safety) first; nothing
  // is measured until the single shared pass below.
  let mut pending: Vec<PendingEntry> = Vec::with_capacity(apps.len() + catalog.entries.len());
  for app in apps {
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
    // Display name and bundle id come from the discovered app; entry id and
    // name come from the catalog when matched.
    let (base_id, name, source) = match matched.first() {
      Some(entry) => (entry.id.clone(), entry.name.clone(), AuditSource::Catalog),
      None => match &app.bundle_id {
        Some(bundle_id) => (bundle_id.clone(), app.name.clone(), AuditSource::Heuristic),
        None => (slugify(&app.name), app.name.clone(), AuditSource::Heuristic),
      },
    };
    pending.push(PendingEntry {
      base_id,
      name,
      kind: AppKind::App,
      source,
      bundle_id: app.bundle_id.clone(),
      app_path: Some(app.app_path.clone()),
      app_canonical: resolve_app_bundle(&app.app_path, &home),
      locations: table.into_inner(),
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
    pending.push(PendingEntry {
      base_id: entry.id.clone(),
      name: entry.name.clone(),
      kind: AppKind::Cli,
      source: AuditSource::Catalog,
      bundle_id: None,
      app_path: None,
      app_canonical: None,
      locations: table.into_inner(),
    });
  }
  let cli_start = pending.len().saturating_sub(
    catalog
      .entries
      .iter()
      .filter(|entry| entry.kind == AppKind::Cli)
      .count(),
  );

  // ONE shared-inode measurement over every surviving location plus every
  // `.app` bundle path, deepest-first so overlapping paths partition.
  let mut roots: Vec<PathBuf> = Vec::new();
  let mut seen_roots: HashSet<PathBuf> = HashSet::new();
  for entry in &pending {
    if let Some(canonical) = &entry.app_canonical {
      if seen_roots.insert(canonical.clone()) {
        roots.push(canonical.clone());
      }
    }
    for path in entry.locations.keys() {
      if seen_roots.insert(path.clone()) {
        roots.push(path.clone());
      }
    }
  }
  let sizes = measure_partitions(&roots);

  // Dedup discovered apps by bundle id, keeping the largest bundle by its
  // measured partition. A dropped duplicate's locations merge into the winner
  // so nested bytes are never lost from the report.
  let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
  for (index, entry) in pending.iter().enumerate().take(cli_start) {
    if let Some(bundle_id) = &entry.bundle_id {
      groups.entry(bundle_id.clone()).or_default().push(index);
    }
  }
  let mut dropped: HashSet<usize> = HashSet::new();
  for members in groups.values() {
    if members.len() < 2 {
      continue;
    }
    let installed = |index: usize| {
      pending[index]
        .app_canonical
        .as_ref()
        .and_then(|path| sizes.get(path))
        .copied()
        .unwrap_or(0)
    };
    let mut winner = members[0];
    let mut best = installed(winner);
    for &member in &members[1..] {
      let size = installed(member);
      if size > best {
        best = size;
        winner = member;
      }
    }
    for &member in members {
      if member == winner {
        continue;
      }
      // A catalog-matched duplicate can lose on bundle size; its curated
      // identity must survive the merge so the entry keeps catalog metadata
      // (and survives `include_heuristic = false`).
      if pending[member].source == AuditSource::Catalog
        && pending[winner].source != AuditSource::Catalog
      {
        pending[winner].base_id = pending[member].base_id.clone();
        pending[winner].name = pending[member].name.clone();
        pending[winner].source = AuditSource::Catalog;
      }
      let loser_locations = std::mem::take(&mut pending[member].locations);
      for (path, meta) in loser_locations {
        pending[winner].locations.entry(path).or_insert(meta);
      }
      dropped.insert(member);
    }
  }
  let mut survivors: Vec<PendingEntry> = pending
    .into_iter()
    .enumerate()
    .filter(|(index, _)| !dropped.contains(index))
    .map(|(_, entry)| entry)
    .collect();

  // Filter BEFORE attributing shared paths: an entry that the filter would
  // drop must not claim a location another surviving entry also owns.
  if !options.include_heuristic {
    survivors.retain(|entry| entry.source == AuditSource::Catalog);
  }

  // Report-level uniqueness: no canonical path appears in two entries.
  // Catalog-owned locations (and catalog-sourced entries) win collisions.
  dedup_locations_across_entries(&mut survivors);

  let mut entries: Vec<AppAuditEntry> = Vec::new();
  let mut used_ids: HashSet<String> = HashSet::new();
  for entry in &survivors {
    let installed_size = entry
      .app_canonical
      .as_ref()
      .and_then(|path| sizes.get(path))
      .copied()
      .unwrap_or(0);
    let mut locations = Vec::with_capacity(entry.locations.len());
    for (path, meta) in &entry.locations {
      // A location identical to its own `.app` bundle would double-count the
      // entry total; the bundle size already covers it.
      if Some(path) == entry.app_canonical.as_ref() {
        continue;
      }
      // Roots that vanished between validation and measurement fail soft to 0
      // and fall out through the same `min_size` filter as any other location.
      let size = sizes.get(path).copied().unwrap_or(0);
      if size < options.min_size {
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
    locations.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    let total_size = installed_size + locations.iter().map(|item| item.size).sum::<u64>();
    if locations.is_empty() && installed_size == 0 {
      continue;
    }
    entries.push(AppAuditEntry {
      id: unique_id(&entry.base_id, &mut used_ids),
      name: entry.name.clone(),
      kind: entry.kind,
      source: entry.source,
      bundle_id: entry.bundle_id.clone(),
      app_path: entry
        .app_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned()),
      installed_size,
      total_size,
      locations,
    });
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

/// Measure many roots in ONE `scan_directory` call, deepest-first (path
/// component count desc, then path asc). Kuntu's `ScanCtx` shares its
/// seen-inode set across all roots of one call, so a hardlinked inode is
/// attributed exactly once (to the deepest root that reaches it) and every
/// parent root's size becomes its own data minus deeper measured roots:
/// locations are additive partitions with zero double counting. Roots that
/// vanished before the scan simply have no returned node.
fn measure_partitions(roots: &[PathBuf]) -> HashMap<PathBuf, u64> {
  let mut sorted = roots.to_vec();
  sorted.sort_by(|a, b| {
    b.components()
      .count()
      .cmp(&a.components().count())
      .then_with(|| a.cmp(b))
  });
  if sorted.is_empty() {
    return HashMap::new();
  }
  let nodes = scan_directory(ScanOptions {
    directories: sorted,
    ignore_hidden: false,
    full_path: true,
    respect_gitignore: false,
    ignored_mode: IgnoredMode::Exclude,
    follow_symlinks: false,
  });
  let mut sizes = HashMap::with_capacity(nodes.len());
  for node in nodes {
    sizes.insert(node.path.clone(), node.size);
  }
  sizes
}

/// Report-level location-path uniqueness: no canonical path may appear in two
/// entries. Catalog entries win over heuristic collisions; otherwise the
/// first entry keeps the path.
fn dedup_locations_across_entries(pending: &mut [PendingEntry]) {
  let mut by_path: HashMap<PathBuf, Vec<usize>> = HashMap::new();
  for (index, entry) in pending.iter().enumerate() {
    for path in entry.locations.keys() {
      by_path.entry(path.clone()).or_default().push(index);
    }
  }
  for (path, mut owners) in by_path.into_iter().filter(|(_, owners)| owners.len() > 1) {
    owners.sort_unstable();
    let mut winner = owners[0];
    let better = |preferred: usize, challenger: usize| {
      let preferred_location = pending[preferred].locations.get(&path);
      let challenger_location = pending[challenger].locations.get(&path);
      // A catalog-owned location beats a heuristic one; on a tie, the entry
      // sourced from the catalog beats a heuristic entry.
      match (
        preferred_location.map(|meta| meta.source),
        challenger_location.map(|meta| meta.source),
      ) {
        (Some(AuditSource::Heuristic), Some(AuditSource::Catalog)) => true,
        (Some(AuditSource::Catalog), Some(AuditSource::Heuristic)) => false,
        _ => {
          pending[challenger].source == AuditSource::Catalog
            && pending[preferred].source != AuditSource::Catalog
        }
      }
    };
    for &candidate in &owners[1..] {
      if better(winner, candidate) {
        winner = candidate;
      }
    }
    for owner in owners {
      if owner != winner {
        pending[owner].locations.remove(&path);
      }
    }
  }
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
      let Some(canonical) = resolve_location(&raw, home) else {
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
      let Some(canonical) = resolve_location(&raw, home) else {
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

  fn into_inner(self) -> HashMap<PathBuf, LocationMeta> {
    self.inner
  }
}

/// Conventional per-bundle-id locations from the plan's table. Apps without a
/// bundle id fall back to name-only candidates. Bundle ids and display names
/// are substituted as single path components: anything containing `/`, `\`,
/// `..`, or empty contributes no candidates.
fn conventional_candidates(name: &str, bundle_id: Option<&str>) -> Vec<(PathBuf, Classification)> {
  use Classification::{Cache, Logs, Support};
  let primary = bundle_id.unwrap_or(name);
  let primary_valid = is_single_component(primary);
  let mut trio: Vec<&str> = Vec::with_capacity(2);
  for key in [primary, name] {
    if is_single_component(key) && !trio.contains(&key) {
      trio.push(key);
    }
  }
  let mut candidates = Vec::new();
  for key in trio {
    candidates.push((format!("Library/Application Support/{key}"), Support));
    candidates.push((format!("Library/Caches/{key}"), Cache));
    candidates.push((format!("Library/Logs/{key}"), Logs));
  }
  if primary_valid {
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
  }
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

/// Bundle ids and display names substituted into candidate paths must be
/// single path components: reject anything containing `/`, `\`, `..`, or
/// empty.
fn is_single_component(value: &str) -> bool {
  !value.is_empty() && !value.contains('/') && !value.contains('\\') && !value.contains("..")
}

/// Fail-closed ancestor walk: every component of `raw` strictly below `home`
/// is checked with `symlink_metadata`; any symlink means the location is
/// skipped entirely, so a catalog child can never traverse a linked storage
/// root (e.g. into Group Containers).
fn has_symlink_ancestor_below_home(raw: &Path, home: &Path) -> bool {
  if !raw.starts_with(home) {
    return false;
  }
  let home_components = home.components().count();
  let raw_components = raw.components().count();
  if raw_components <= home_components {
    return false;
  }
  for count in (home_components + 1)..=raw_components {
    let prefix: PathBuf = raw.components().take(count).collect();
    if let Ok(metadata) = std::fs::symlink_metadata(&prefix) {
      if metadata.file_type().is_symlink() {
        return true;
      }
    }
  }
  false
}

fn contains_group_containers(path: &Path) -> bool {
  path
    .components()
    .any(|component| component.as_os_str() == "Group Containers")
}

/// Resolve a location path to its canonical absolute form, fail-closed:
/// symlinked ancestors below home, symlinked leaves, missing paths, `Group
/// Containers` destinations, and paths escaping home are all skipped
/// (measurement never follows links).
fn resolve_location(raw: &Path, home: &Path) -> Option<PathBuf> {
  if has_symlink_ancestor_below_home(raw, home) {
    return None;
  }
  match std::fs::symlink_metadata(raw) {
    Ok(metadata) if metadata.file_type().is_symlink() => return None,
    Ok(_) => {}
    Err(_) => return None,
  }
  let canonical = std::fs::canonicalize(raw).ok()?;
  if contains_group_containers(&canonical) {
    return None;
  }
  if !canonical.starts_with(home) {
    return None;
  }
  Some(canonical)
}

/// Resolve a `.app` bundle path for measurement. Bundles may live outside
/// home (`/Applications`), so the ancestor walk starts at the filesystem
/// root, not at home: any symlinked component anywhere above the bundle
/// rejects it. The symlinked-leaf and Group Containers gates still apply;
/// rejected bundles measure as 0.
fn resolve_app_bundle(path: &Path, home: &Path) -> Option<PathBuf> {
  if has_symlink_ancestor_below_home(path, home) {
    return None;
  }
  let mut prefix = PathBuf::new();
  for component in path.components() {
    prefix.push(component);
    if let Ok(metadata) = std::fs::symlink_metadata(&prefix) {
      if metadata.file_type().is_symlink() {
        return None;
      }
    }
  }
  match std::fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_symlink() => return None,
    Ok(_) => {}
    Err(_) => return None,
  }
  let canonical = std::fs::canonicalize(path).ok()?;
  if contains_group_containers(&canonical) {
    return None;
  }
  Some(canonical)
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
  fn partition_parent_excludes_measured_children() {
    let fixture = Fixture::new("partition");
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
    // Partition semantics: no residual entries are generated; the parent keeps
    // its own catalog label and classification.
    assert!(
      !entry
        .locations
        .iter()
        .any(|item| item.label.as_deref() == Some("Other data")),
      "residual entry leaked into partitions"
    );
    let child = entry
      .locations
      .iter()
      .find(|item| item.path == sessions)
      .expect("sessions child");
    let parent = entry
      .locations
      .iter()
      .find(|item| item.path == root)
      .expect("data root");
    assert_eq!(parent.label.as_deref(), Some("Codex data root"));
    assert_eq!(parent.classification, Classification::Support);
    assert!(!parent.purgeable);
    assert_eq!(parent.source, AuditSource::Catalog);
    let parent_total = space_lens::measure_path(&home.join(".codex"));
    let child_total = space_lens::measure_path(&home.join(".codex/sessions"));
    // Deepest root keeps its full size; the parent keeps its total minus the
    // measured child.
    assert_eq!(child.size, child_total);
    assert_eq!(parent.size, parent_total.saturating_sub(child_total));
    // No double count: the entry total equals the measured parent total.
    assert_eq!(entry.total_size, parent_total);
  }

  /// Three-level nesting through the shared pass: P superset P/A superset
  /// P/A/B partitions so the three sizes sum to P's true byte total.
  /// Unix-only: Windows dir inodes are never deduped (see
  /// `inode_key`), so ancestor directory-entry blocks recount there.
  #[cfg(unix)]
  #[test]
  fn partition_three_level_nesting() {
    let fixture = Fixture::new("nesting");
    let home = fixture.home();
    fixture.file("nest/outer.bin", 1_048_576);
    fixture.file("nest/A/mid.bin", 1_048_576);
    fixture.file("nest/A/B/inner.bin", 1_048_576);
    let parent = home.join("nest");
    let middle = parent.join("A");
    let inner = middle.join("B");
    // Input order must not matter: pass shallowest-first, the shared pass
    // sorts deepest-first itself.
    let sizes = measure_partitions(&[parent.clone(), middle.clone(), inner.clone()]);
    assert_eq!(sizes.len(), 3);
    let parent_total = space_lens::measure_path(&parent);
    let middle_total = space_lens::measure_path(&middle);
    let inner_total = space_lens::measure_path(&inner);
    assert_eq!(sizes[&inner], inner_total);
    assert_eq!(sizes[&middle], middle_total.saturating_sub(inner_total));
    assert_eq!(sizes[&parent], parent_total.saturating_sub(middle_total));
    assert_eq!(sizes.values().sum::<u64>(), parent_total);
    // One ~1 MiB file per level: every partition keeps a non-trivial share.
    for size in sizes.values() {
      assert!(*size >= 1_000_000, "partition too small: {size}");
    }
  }

  /// A file hardlinked into two sibling locations is attributed exactly once:
  /// the two partition sizes sum to the unique bytes.
  #[test]
  fn partition_hardlink_counted_once() {
    let fixture = Fixture::new("hardlink");
    let home = fixture.home();
    fixture.file("link-a/blob.bin", 1_048_576);
    let source = home.join("link-a/blob.bin");
    let dir_b = home.join("link-b");
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::hard_link(&source, dir_b.join("blob.bin")).unwrap();
    let dir_a = home.join("link-a");
    let sizes = measure_partitions(&[dir_a.clone(), dir_b.clone()]);
    assert_eq!(sizes.len(), 2);
    let solo_a = space_lens::measure_path(&dir_a);
    let solo_b = space_lens::measure_path(&dir_b);
    #[cfg(unix)]
    let file_alloc = {
      use std::os::unix::fs::MetadataExt;
      std::fs::symlink_metadata(&source).unwrap().blocks() * 512
    };
    #[cfg(not(unix))]
    let file_alloc = std::fs::symlink_metadata(&source).unwrap().len();
    // Equal depth breaks ties by path ascending, so link-a is measured first
    // and keeps the shared inode; link-b keeps only its own directory blocks.
    assert_eq!(sizes[&dir_a], solo_a);
    assert_eq!(sizes[&dir_b], solo_b.saturating_sub(file_alloc));
    assert_eq!(sizes.values().sum::<u64>(), solo_a + solo_b - file_alloc);
  }

  /// Two entries resolving to one canonical child path: the child is measured
  /// once and attributed to one entry only (catalog entry wins), and no
  /// canonical path appears in two entries anywhere in the report.
  #[test]
  fn shared_child_attributed_once_across_entries() {
    let fixture = Fixture::new("shared");
    let home = fixture.home();
    // Raycast's catalog data root ...
    fixture.file(
      "Library/Application Support/com.raycast.macos/data.db",
      4096,
    );
    // ... collides with a heuristic candidate of an unrelated app whose
    // display name happens to equal that bundle id, which also owns a
    // disjoint location so both entries survive.
    fixture.file(
      "Library/Application Support/com.example.Other/data.bin",
      4096,
    );

    let apps = vec![
      discovered("Raycast", Some("com.raycast.macos"), &home),
      DiscoveredApp {
        name: "com.raycast.macos".to_string(),
        bundle_id: Some("com.example.Other".to_string()),
        app_path: home.join("Applications/Other.app"),
      },
    ];
    let report = audit_with(&apps, &home, &options(0, true, 500)).unwrap();
    let shared = home
      .join("Library/Application Support/com.raycast.macos")
      .to_string_lossy()
      .into_owned();
    let owners: Vec<&AppAuditEntry> = report
      .apps
      .iter()
      .filter(|entry| {
        entry
          .locations
          .iter()
          .any(|location| location.path == shared)
      })
      .collect();
    assert_eq!(
      owners.len(),
      1,
      "shared path attributed to {} entries",
      owners.len()
    );
    assert_eq!(owners[0].id, "raycast");
    assert_eq!(
      owners[0]
        .locations
        .iter()
        .find(|location| location.path == shared)
        .unwrap()
        .source,
      AuditSource::Catalog
    );
    assert!(
      report
        .apps
        .iter()
        .any(|entry| entry.bundle_id.as_deref() == Some("com.example.Other")),
      "heuristic entry lost its disjoint location"
    );
    let mut seen = HashSet::new();
    for entry in &report.apps {
      for location in &entry.locations {
        assert!(
          seen.insert(location.path.clone()),
          "path in two entries: {}",
          location.path
        );
      }
    }
  }

  #[test]
  fn filtered_entries_do_not_claim_shared_paths() {
    let fixture = Fixture::new("filtered-claim");
    let home = fixture.home();
    // VS Code's catalog location collides with a heuristic candidate of an
    // unrelated app literally named "Code". With heuristic entries filtered
    // out, the unrelated app must not have claimed the path first.
    fixture.file("Library/Application Support/Code/User/settings.json", 4096);

    let apps = vec![
      DiscoveredApp {
        name: "Code".to_string(),
        bundle_id: None,
        app_path: home.join("Applications/Code.app"),
      },
      discovered("Visual Studio Code", Some("com.microsoft.VSCode"), &home),
    ];
    let report = audit_with(&apps, &home, &options(0, false, 500)).unwrap();
    let code = home
      .join("Library/Application Support/Code")
      .to_string_lossy()
      .into_owned();
    let vscode = report
      .apps
      .iter()
      .find(|entry| entry.id == "vscode")
      .expect("vscode entry survives include_heuristic=false");
    assert!(
      vscode
        .locations
        .iter()
        .any(|location| location.path == code),
      "catalog entry lost its claimed path to a filtered heuristic entry"
    );
  }

  #[test]
  fn catalog_identity_survives_bundle_dedup() {
    let fixture = Fixture::new("dedup-identity");
    let home = fixture.home();
    // A bigger unmatched bundle ("VSCode Beta") shares the bundle id with a
    // smaller catalog-matched install. Dedup keeps the bigger bundle but must
    // carry the catalog identity, so it survives include_heuristic=false.
    fixture.file("Applications/VSCode Beta.app/Contents/big.bin", 512 * 1024);
    fixture.file(
      "Applications/Visual Studio Code.app/Contents/small.bin",
      4096,
    );
    fixture.file("Library/Application Support/Code/User/settings.json", 4096);

    let apps = vec![
      discovered("Visual Studio Code", Some("com.microsoft.VSCode"), &home),
      DiscoveredApp {
        name: "VSCode Beta".to_string(),
        bundle_id: Some("com.microsoft.VSCode".to_string()),
        app_path: home.join("Applications/VSCode Beta.app"),
      },
    ];
    let report = audit_with(&apps, &home, &options(0, false, 500)).unwrap();
    assert_eq!(report.apps.len(), 1, "duplicate bundle ids must merge");
    assert_eq!(report.apps[0].id, "vscode");
    assert_eq!(report.apps[0].source, AuditSource::Catalog);
    // The surviving entry reports the larger bundle it kept.
    assert!(report.apps[0].installed_size >= 512 * 1024);
  }

  /// A bundle reached through a symlinked ancestor OUTSIDE home (e.g. a
  /// linked /Applications subfolder) is rejected, not measured.
  #[cfg(unix)]
  #[test]
  fn bundle_behind_outside_home_symlink_ancestor_measures_zero() {
    let fixture = Fixture::new("bundle-symlink");
    let home = fixture.home();
    let real_apps = fixture.root.join("real-apps");
    std::fs::create_dir_all(real_apps.join("Linked.app/Contents/MacOS")).unwrap();
    std::fs::write(
      real_apps.join("Linked.app/Contents/MacOS/binary"),
      vec![0u8; 256 * 1024],
    )
    .unwrap();
    let link = fixture.root.join("linked-apps");
    std::os::unix::fs::symlink(&real_apps, &link).unwrap();

    let apps = vec![DiscoveredApp {
      name: "Linked".to_string(),
      bundle_id: Some("com.example.Linked".to_string()),
      app_path: link.join("Linked.app"),
    }];
    let report = audit_with(&apps, &home, &options(0, true, 500)).unwrap();
    assert!(
      report.apps.is_empty(),
      "symlink-reached bundle must not be measured"
    );
  }

  /// A catalog child below a symlinked storage root is skipped fail-closed.
  /// Unix-only: creating symlinks needs privileges on Windows.
  #[cfg(unix)]
  #[test]
  fn ancestor_symlink_location_skipped() {
    let fixture = Fixture::new("ancestor-symlink");
    let home = fixture.home();
    // Real data lives outside the catalog path; ~/.codex is a symlink to it.
    let target = fixture.root.join("real-codex");
    std::fs::create_dir_all(target.join("sessions")).unwrap();
    std::fs::write(
      target.join("sessions").join("a.jsonl"),
      "{\"msg\":\"hi\"}\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&target, home.join(".codex")).unwrap();

    let report = audit_with(&[], &home, &options(0, true, 500)).unwrap();
    assert!(
      !report.apps.iter().any(|app| app.id == "codex-cli"),
      "location below a symlinked ancestor must be skipped"
    );
  }

  /// `Group Containers` destinations are rejected and hostile bundle ids /
  /// display names contribute no candidates.
  #[test]
  fn group_containers_and_hostile_components_rejected() {
    let fixture = Fixture::new("path-safety");
    let home = fixture.home();
    fixture.file("Library/Group Containers/com.example.Foo/data.bin", 4096);
    assert!(
      resolve_location(
        &home.join("Library/Group Containers/com.example.Foo"),
        &home
      )
      .is_none(),
      "Group Containers destination must be rejected"
    );

    assert!(is_single_component("com.example.Foo"));
    assert!(!is_single_component(""));
    assert!(!is_single_component("a/b"));
    assert!(!is_single_component("a\\b"));
    assert!(!is_single_component("a..b"));
    assert!(!is_single_component(".."));

    // A bundle id with path separators yields no candidates on its own, and
    // the entry disappears when nothing else resolves.
    let apps = vec![discovered("Evil", Some("evil/../escape"), &home)];
    let report = audit_with(&apps, &home, &options(0, true, 500)).unwrap();
    assert!(
      report.apps.is_empty(),
      "hostile bundle id must contribute no locations"
    );
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
    // ~1 MiB allocated, the small log is a single block, and parent
    // partitions shrink to bare directory metadata once children are measured.
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
