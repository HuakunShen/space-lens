#![deny(clippy::all)]

#[macro_use]
extern crate napi_derive;

use du_dust::{self, display_node::DisplayNode as DuDisplayNode, Node as DuNode};
use std::path::PathBuf;

#[napi(object)]
pub struct Node {
  pub name: String,
  #[napi(js_name = "size")]
  pub size: i64,
  pub children: Vec<Node>,
  pub depth: u32,
}

impl From<DuNode> for Node {
  fn from(node: DuNode) -> Self {
    Node {
      name: node.name.to_string_lossy().to_string(),
      size: node.size as i64,
      children: node.children.into_iter().map(Node::from).collect(),
      depth: node.depth as u32,
    }
  }
}

#[napi(object)]
pub struct DisplayNode {
  pub name: String,
  #[napi(js_name = "size")]
  pub size: i64,
  pub children: Vec<DisplayNode>,
}

impl From<DuDisplayNode> for DisplayNode {
  fn from(node: DuDisplayNode) -> Self {
    DisplayNode {
      name: node.name.to_string_lossy().to_string(),
      size: node.size as i64,
      children: node.children.into_iter().map(DisplayNode::from).collect(),
    }
  }
}

#[napi]
pub fn build_directory_tree(directories: Vec<String>, ignore_hidden: bool) -> Vec<Node> {
  du_dust::build_directory_tree(directories, ignore_hidden)
    .into_iter()
    .map(Node::from)
    .collect()
}

#[napi]
pub fn get_largest_nodes(nodes: Vec<Node>, number_of_nodes: u32) -> Option<DisplayNode> {
  // Convert our NAPI Nodes back to du_dust Nodes
  let du_nodes: Vec<DuNode> = nodes
    .into_iter()
    .map(|node| DuNode {
      name: PathBuf::from(node.name),
      size: node.size as u64,
      children: node
        .children
        .into_iter()
        .map(|child| DuNode {
          name: PathBuf::from(child.name),
          size: child.size as u64,
          children: vec![],
          inode_device: None,
          depth: child.depth as usize,
        })
        .collect(),
      inode_device: None,
      depth: node.depth as usize,
    })
    .collect();

  // Call the du_dust function and convert the result to our NAPI type
  du_dust::get_largest_nodes(du_nodes, number_of_nodes as usize).map(DisplayNode::from)
}
