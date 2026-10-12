//! Curated per-platform storage catalog, embedded in the binary.
//!
//! The catalog adds known non-conventional locations (CLI tool dot-dirs,
//! name-aliased dirs like `Application Support/Code`), classifications, and
//! human explanations to the auto-discovered heuristic locations.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{AppKind, Classification};

/// Embedded macOS catalog source.
// TODO(remote-catalog): fetch catalogs/*.json from the space-lens GitHub repo,
// cache under ~/Library/Application Support, refresh max once/day, and fall
// back to this embedded copy.
const MACOS_JSON: &str = include_str!("../catalogs/macos.json");

static MACOS_CATALOG: OnceLock<Catalog> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
  pub schema_version: u32,
  pub platform: String,
  pub entries: Vec<CatalogEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogEntry {
  pub id: String,
  pub name: String,
  pub kind: AppKind,
  pub r#match: Option<CatalogMatch>,
  pub locations: Vec<CatalogLocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogMatch {
  pub bundle_ids: Vec<String>,
  pub app_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogLocation {
  pub path: String,
  pub label: Option<String>,
  pub classification: Classification,
  pub purgeable: bool,
  pub comment: Option<String>,
}

/// The parsed macOS catalog. Panics only when the embedded file itself is
/// invalid, which is a build-time bug rather than a runtime condition.
pub fn macos_catalog() -> &'static Catalog {
  MACOS_CATALOG.get_or_init(|| {
    load_catalog().unwrap_or_else(|error| panic!("embedded macos catalog is invalid: {error}"))
  })
}

pub(crate) fn load_catalog() -> Result<Catalog, String> {
  let catalog: Catalog =
    serde_json::from_str(MACOS_JSON).map_err(|error| format!("macos catalog: {error}"))?;
  validate_catalog(&catalog)?;
  Ok(catalog)
}

pub(crate) fn validate_catalog(catalog: &Catalog) -> Result<(), String> {
  let mut ids = HashSet::new();
  for entry in &catalog.entries {
    if !ids.insert(entry.id.as_str()) {
      return Err(format!("duplicate catalog id: {}", entry.id));
    }
    if entry.locations.is_empty() {
      return Err(format!("catalog entry '{}' has no locations", entry.id));
    }
    match (&entry.kind, &entry.r#match) {
      (AppKind::App, None) => {
        return Err(format!("app-kind entry '{}' has null match", entry.id));
      }
      (AppKind::Cli, Some(_)) => {
        return Err(format!("cli entry '{}' must have null match", entry.id));
      }
      _ => {}
    }
    for location in &entry.locations {
      if !location.path.starts_with("~/") {
        return Err(format!(
          "catalog location path must start with ~/: {}",
          location.path
        ));
      }
    }
  }
  Ok(())
}
