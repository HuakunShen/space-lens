//! Collector staging, plan summarization, and fingerprint re-verification —
//! pure functions over plain data: no GPUI types, no filesystem access, so
//! the UI wiring (later milestones) stays thin and this layer stays testable.

use std::time::{SystemTime, UNIX_EPOCH};

use space_lens::RemovalEntry;

/// A path staged in the collector. Only the fields the dedup rules need;
/// the views layer wraps this with display data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedPath {
  pub node_id: String,
  pub path: String,
}

/// Stages `entry` with the web workbench's `collect` semantics
/// (`apps/web/src/routes/+page.svelte:241-257`): a repeated node id is a
/// no-op, and a newly staged ancestor supersedes its already-staged
/// descendants — matched by `entry.path + "/"` prefix so siblings that merely
/// share a string prefix (`/a/bc` vs `/a/b`) are not conflated. Staging a
/// descendant under an already-staged ancestor is allowed, matching the web
/// behavior; the trash step tolerates the overlap via its failure list.
pub fn stage_entry(staged: &mut Vec<StagedPath>, entry: StagedPath) {
  if staged.iter().any(|staged| staged.node_id == entry.node_id) {
    return;
  }
  let descendants_prefix = format!("{}/", entry.path);
  staged.retain(|staged| !staged.path.starts_with(&descendants_prefix));
  staged.push(entry);
}

/// File identity captured at plan time and re-checked at execute time;
/// any mismatch fails the whole plan (engine.rs:512-531).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntryFingerprint {
  pub size: u64,
  pub mtime_ms: f64,
}

/// mtime in milliseconds since the epoch; `0.0` when the mtime is
/// unavailable or predates the epoch — engine.rs:472-477 semantics. Takes
/// the already-extracted `modified()` so stat handling stays with the caller.
pub fn mtime_ms(modified: Option<SystemTime>) -> f64 {
  modified
    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    .map(|duration| duration.as_millis() as f64)
    .unwrap_or(0.0)
}

/// One-mismatch-fails-the-plan check (engine.rs:512-531): each pair is a
/// planned fingerprint and the just-stat'd current state of the same path;
/// `current == None` means the path no longer stats (moved, deleted, or
/// unreadable) and counts as changed. `Err(changed)` rejects the whole plan.
pub fn verify_fingerprints(
  pairs: &[(EntryFingerprint, Option<EntryFingerprint>)],
) -> Result<(), usize> {
  let changed = pairs
    .iter()
    .filter(|(planned, current)| match current {
      Some(current) => *current != *planned,
      None => true,
    })
    .count();
  if changed == 0 {
    Ok(())
  } else {
    Err(changed)
  }
}

/// Counts and bytes for the confirm dialog (plan §5), summed the same way
/// kuntu's `build_removal_plan` totals its entries (clean.rs:127).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanSummary {
  pub entry_count: usize,
  pub total_size: u64,
}

pub fn summarize(entries: &[RemovalEntry]) -> PlanSummary {
  PlanSummary {
    entry_count: entries.len(),
    total_size: entries.iter().map(|entry| entry.size).sum(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn staged(id: &str, path: &str) -> StagedPath {
    StagedPath {
      node_id: id.to_string(),
      path: path.to_string(),
    }
  }

  #[test]
  fn staging_the_same_node_twice_is_a_no_op() {
    let mut staged_paths = vec![staged("n1", "/a")];
    stage_entry(&mut staged_paths, staged("n1", "/a"));
    assert_eq!(staged_paths, vec![staged("n1", "/a")]);
  }

  #[test]
  fn a_new_ancestor_supersedes_staged_descendants() {
    let mut staged_paths = vec![staged("b", "/a/b"), staged("c", "/c")];
    stage_entry(&mut staged_paths, staged("a", "/a"));
    assert_eq!(staged_paths, vec![staged("c", "/c"), staged("a", "/a")]);
  }

  #[test]
  fn shared_string_prefix_is_not_descendance() {
    // /a/bc is a sibling of /a/b, not its child: staging /a/b must keep it.
    let mut staged_paths = vec![staged("bc", "/a/bc")];
    stage_entry(&mut staged_paths, staged("b", "/a/b"));
    assert_eq!(
      staged_paths,
      vec![staged("bc", "/a/bc"), staged("b", "/a/b")]
    );
  }

  #[test]
  fn staging_a_descendant_keeps_the_staged_ancestor() {
    // Web workbench behavior: only the ancestor→descendant direction
    // replaces entries; the trash step's failure list absorbs the overlap.
    let mut staged_paths = vec![staged("a", "/a")];
    stage_entry(&mut staged_paths, staged("b", "/a/b"));
    assert_eq!(staged_paths, vec![staged("a", "/a"), staged("b", "/a/b")]);
  }

  #[test]
  fn fingerprints_match_only_on_size_and_mtime() {
    let planned = EntryFingerprint {
      size: 10,
      mtime_ms: 100.0,
    };
    let ok = verify_fingerprints(&[(planned, Some(planned))]);
    assert_eq!(ok, Ok(()));

    let size_changed = EntryFingerprint {
      size: 11,
      mtime_ms: 100.0,
    };
    let mtime_changed = EntryFingerprint {
      size: 10,
      mtime_ms: 200.0,
    };
    assert_eq!(
      verify_fingerprints(&[(planned, Some(size_changed))]),
      Err(1)
    );
    assert_eq!(
      verify_fingerprints(&[(planned, Some(mtime_changed))]),
      Err(1)
    );
    // A vanished path counts as changed.
    assert_eq!(verify_fingerprints(&[(planned, None)]), Err(1));
    // Every mismatch is reported; an all-clean plan verifies.
    let pairs = [
      (planned, Some(planned)),
      (planned, Some(size_changed)),
      (planned, None),
    ];
    assert_eq!(verify_fingerprints(&pairs), Err(2));
    assert_eq!(verify_fingerprints(&[]), Ok(()));
  }

  #[test]
  fn mtime_converts_millis_and_falls_back_to_zero() {
    assert_eq!(mtime_ms(None), 0.0);
    assert_eq!(mtime_ms(Some(UNIX_EPOCH)), 0.0);
    assert_eq!(
      mtime_ms(Some(UNIX_EPOCH + std::time::Duration::from_millis(1234))),
      1234.0
    );
  }

  #[test]
  fn summarize_counts_entries_and_sums_sizes() {
    let entry = |size| RemovalEntry {
      path: format!("/p/{size}").into(),
      size,
      reason: "staged".into(),
      preset: space_lens::CleanupPreset::Node,
    };
    assert_eq!(
      summarize(&[]),
      PlanSummary {
        entry_count: 0,
        total_size: 0
      }
    );
    let summary = summarize(&[entry(100), entry(23)]);
    assert_eq!(
      summary,
      PlanSummary {
        entry_count: 2,
        total_size: 123
      }
    );
  }
}
