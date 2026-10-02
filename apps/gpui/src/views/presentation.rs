//! Presentation state for the native workbench, independent of scan data.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTab {
  #[default]
  Contents,
  Cleanup,
  Git,
}

pub struct WorkbenchPresentation {
  pub inspector_visible: bool,
  pub inspector_tab: InspectorTab,
  pub collector_visible: bool,
}

impl Default for WorkbenchPresentation {
  fn default() -> Self {
    Self {
      inspector_visible: true,
      inspector_tab: InspectorTab::Contents,
      collector_visible: false,
    }
  }
}

impl WorkbenchPresentation {
  pub fn select_inspector(&mut self, tab: InspectorTab) {
    self.inspector_tab = tab;
    self.inspector_visible = true;
  }

  pub fn staged_changed(&mut self, count: usize) {
    if count > 0 {
      self.collector_visible = true;
    }
  }

  pub fn scan_completed(&mut self) {
    self.inspector_tab = InspectorTab::Contents;
    self.collector_visible = false;
  }

  pub fn cleanup_finished(&mut self) {
    self.collector_visible = true;
  }
}

#[cfg(test)]
mod tests {
  use super::{InspectorTab, WorkbenchPresentation};

  #[test]
  fn choosing_cleanup_reopens_a_hidden_inspector() {
    let mut view = WorkbenchPresentation::default();
    view.inspector_visible = false;
    view.select_inspector(InspectorTab::Cleanup);
    assert!(view.inspector_visible);
    assert_eq!(view.inspector_tab, InspectorTab::Cleanup);
  }

  #[test]
  fn collecting_a_path_reveals_the_review_tray() {
    let mut view = WorkbenchPresentation::default();
    view.staged_changed(1);
    assert!(view.collector_visible);
  }

  #[test]
  fn empty_queue_keeps_the_cleanup_outcome_visible() {
    let mut view = WorkbenchPresentation::default();
    view.collector_visible = true;
    view.staged_changed(0);
    assert!(view.collector_visible);
  }

  #[test]
  fn a_new_scan_resets_stale_collector_and_inspector_views() {
    let mut view = WorkbenchPresentation::default();
    view.collector_visible = true;
    view.inspector_tab = InspectorTab::Git;
    view.scan_completed();
    assert!(!view.collector_visible);
    assert_eq!(view.inspector_tab, InspectorTab::Contents);
  }

  #[test]
  fn cleanup_finishing_after_a_new_scan_reveals_its_report() {
    let mut view = WorkbenchPresentation::default();
    view.staged_changed(1);
    view.scan_completed();
    view.staged_changed(0);
    view.cleanup_finished();
    assert!(view.collector_visible);
  }
}
