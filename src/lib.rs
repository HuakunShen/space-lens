#![deny(clippy::all)]

use napi_derive::napi;

mod scanner;

use scanner::{IgnoredMode, ScanNode, ScanOptions};

#[napi(object)]
pub struct DirectoryScanOptions {
  pub directories: Vec<String>,
  #[napi(js_name = "ignoreHidden")]
  pub ignore_hidden: Option<bool>,
  #[napi(js_name = "fullPath")]
  pub full_path: Option<bool>,
  #[napi(js_name = "respectGitignore")]
  pub respect_gitignore: Option<bool>,
  #[napi(js_name = "ignoredMode")]
  pub ignored_mode: Option<String>,
}

#[napi(object)]
pub struct DirectoryNode {
  pub name: String,
  #[napi(js_name = "size")]
  pub size: i64,
  pub children: Vec<DirectoryNode>,
  pub depth: u32,
  pub ignored: bool,
  pub collapsed: bool,
}

impl From<ScanNode> for DirectoryNode {
  fn from(node: ScanNode) -> Self {
    DirectoryNode {
      name: node.name,
      size: node.size as i64,
      children: node.children.into_iter().map(DirectoryNode::from).collect(),
      depth: node.depth,
      ignored: node.ignored,
      collapsed: node.collapsed,
    }
  }
}

#[napi(js_name = "scanDirectory")]
pub fn scan_directory(options: DirectoryScanOptions) -> Vec<DirectoryNode> {
  let ignored_mode = match options.ignored_mode.as_deref() {
    Some("exclude") => IgnoredMode::Exclude,
    _ => IgnoredMode::Summarize,
  };

  scanner::scan_directory(ScanOptions {
    directories: options.directories,
    ignore_hidden: options.ignore_hidden.unwrap_or(false),
    full_path: options.full_path.unwrap_or(false),
    respect_gitignore: options.respect_gitignore.unwrap_or(true),
    ignored_mode,
  })
  .into_iter()
  .map(DirectoryNode::from)
  .collect()
}
