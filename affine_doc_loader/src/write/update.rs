//! Update YDoc module
//!
//! Provides functionality to update existing AFFiNE documents by applying
//! surgical y-octo operations based on content differences.

use std::collections::HashMap;

use super::{
  super::{
    block_spec::{TreeNode, count_tree_nodes, text_delta_eq},
    blocksuite::{collect_child_ids, find_child_id_by_flavour},
    markdown::{MAX_BLOCKS, parse_markdown_blocks},
  },
  builder::{ApplyBlockOptions, apply_block_spec, insert_block_tree, insert_children},
  *,
};

const MAX_LCS_CELLS: usize = 2_000_000;

#[derive(Debug, Clone)]
struct StoredNode {
  id: String,
  spec: BlockSpec,
  children: Vec<StoredNode>,
}

impl TreeNode for StoredNode {
  fn children(&self) -> &[StoredNode] {
    &self.children
  }
}

struct DocState {
  doc: Doc,
  note_id: String,
  blocks: Vec<StoredNode>,
}

#[derive(Debug)]
enum PatchOp {
  Keep(usize, usize),
  Delete(usize),
  Insert(usize),
  Update(usize, usize),
}

/// Updates an existing document with new markdown content.
///
/// This function performs structural diffing between the existing document
/// and the new markdown content, then applies block-level replacements
/// for changed blocks. This enables proper CRDT merging with concurrent
/// edits from other clients.
///
/// # Arguments
/// * `existing_binary` - The current document binary
/// * `new_markdown` - The new markdown content (document title is not updated)
/// * `doc_id` - The document ID
///
/// # Returns
/// A binary vector representing only the delta (changes) to apply
pub fn update_doc(existing_binary: &[u8], new_markdown: &str, doc_id: &str) -> Result<Vec<u8>, ParseError> {
  let mut new_nodes = parse_markdown_blocks(new_markdown)?;
  let state = load_doc_state(existing_binary, doc_id)?;

  check_limits(&state.blocks, &new_nodes)?;

  let state_before = state.doc.get_state_vector();

  let mut blocks_map = state.doc.get_map("blocks")?;

  let new_children = sync_nodes(&state.doc, &mut blocks_map, &state.blocks, &mut new_nodes)?;
  sync_children(&state.doc, &mut blocks_map, &state.note_id, &new_children)?;

  Ok(state.doc.encode_state_as_update_v1(&state_before)?)
}

fn load_doc_state(binary: &[u8], doc_id: &str) -> Result<DocState, ParseError> {
  let doc = load_doc(binary, Some(doc_id))?;

  let blocks_map = doc.get_map("blocks")?;
  if blocks_map.is_empty() {
    return Err(ParseError::ParserError("blocks map is empty".into()));
  }

  let block_index = build_block_index(&blocks_map);
  let page_id = find_block_id_by_flavour(&block_index.block_pool, PAGE_FLAVOUR)
    .ok_or_else(|| ParseError::ParserError("page block not found".into()))?;
  let page_block = block_index
    .block_pool
    .get(&page_id)
    .ok_or_else(|| ParseError::ParserError("page block not found".into()))?;
  let note_id = find_child_id_by_flavour(page_block, &block_index.block_pool, NOTE_FLAVOUR)
    .ok_or_else(|| ParseError::ParserError("note block not found".into()))?;
  let note_block = block_index
    .block_pool
    .get(&note_id)
    .ok_or_else(|| ParseError::ParserError("note block not found".into()))?;
  let content_ids = collect_child_ids(note_block);

  let mut blocks = Vec::new();
  for block_id in content_ids {
    let block = block_index
      .block_pool
      .get(&block_id)
      .ok_or_else(|| ParseError::ParserError("content block not found".into()))?;
    blocks.push(build_stored_tree(&block_id, block, &block_index.block_pool)?);
  }

  Ok(DocState { doc, note_id, blocks })
}

fn build_stored_tree(block_id: &str, block: &Map, pool: &HashMap<String, Map>) -> Result<StoredNode, ParseError> {
  let spec = BlockSpec::from_block_map(block)?;

  let child_ids = collect_child_ids(block);
  if !child_ids.is_empty() && !matches!(spec.flavour, BlockFlavour::List | BlockFlavour::Callout) {
    return Err(ParseError::ParserError(format!(
      "unsupported children on block: {block_id}"
    )));
  }
  let mut children = Vec::new();
  for child_id in child_ids {
    let child_block = pool
      .get(&child_id)
      .ok_or_else(|| ParseError::ParserError("child block not found".into()))?;
    children.push(build_stored_tree(&child_id, child_block, pool)?);
  }

  Ok(StoredNode {
    id: block_id.to_string(),
    spec,
    children,
  })
}

fn sync_nodes(
  doc: &Doc,
  blocks_map: &mut Map,
  current: &[StoredNode],
  target: &mut [BlockNode],
) -> Result<Vec<String>, ParseError> {
  let ops = diff_blocks(current, target);
  let mut new_children = Vec::new();
  let mut to_remove = Vec::new();

  for op in ops {
    match op {
      PatchOp::Keep(old_idx, new_idx) => {
        let old_node = &current[old_idx];
        let new_node = &target[new_idx];
        update_block_props(doc, blocks_map, old_node, &new_node.spec, true)?;
        let child_ids = sync_nodes(doc, blocks_map, &old_node.children, &mut new_node.children.clone())?;
        sync_children(doc, blocks_map, &old_node.id, &child_ids)?;
        new_children.push(old_node.id.clone());
      }
      PatchOp::Update(old_idx, new_idx) => {
        let old_node = &current[old_idx];
        let new_node = &target[new_idx];
        update_block_props(doc, blocks_map, old_node, &new_node.spec, false)?;
        let child_ids = sync_nodes(doc, blocks_map, &old_node.children, &mut new_node.children.clone())?;
        sync_children(doc, blocks_map, &old_node.id, &child_ids)?;
        new_children.push(old_node.id.clone());
      }
      PatchOp::Insert(new_idx) => {
        let new_id = insert_block_tree(doc, blocks_map, &target[new_idx])?;
        new_children.push(new_id);
      }
      PatchOp::Delete(old_idx) => {
        let node = &current[old_idx];
        if node.spec.flavour == BlockFlavour::Callout {
          new_children.push(node.id.clone());
        } else {
          collect_tree_ids(node, &mut to_remove);
        }
      }
    }
  }

  for id in to_remove {
    blocks_map.remove(&id);
  }

  Ok(new_children)
}

fn diff_blocks(current: &[StoredNode], target: &[BlockNode]) -> Vec<PatchOp> {
  let old_len = current.len();
  let new_len = target.len();

  if old_len == 0 {
    return (0..new_len).map(PatchOp::Insert).collect();
  }
  if new_len == 0 {
    return (0..old_len).map(PatchOp::Delete).collect();
  }

  let mut lcs = vec![vec![0usize; new_len + 1]; old_len + 1];

  for i in 1..=old_len {
    for j in 1..=new_len {
      let old_spec = &current[i - 1].spec;
      let new_spec = &target[j - 1].spec;

      if old_spec.is_exact(new_spec) {
        lcs[i][j] = lcs[i - 1][j - 1] + 1;
      } else {
        lcs[i][j] = std::cmp::max(lcs[i - 1][j], lcs[i][j - 1]);
      }
    }
  }

  let mut ops = Vec::new();
  let mut i = old_len;
  let mut j = new_len;

  while i > 0 || j > 0 {
    if i > 0 && j > 0 {
      let old_spec = &current[i - 1].spec;
      let new_spec = &target[j - 1].spec;

      if old_spec.is_exact(new_spec) {
        ops.push(PatchOp::Keep(i - 1, j - 1));
        i -= 1;
        j -= 1;
      } else if old_spec.is_similar(new_spec)
        && lcs[i - 1][j - 1] >= lcs[i - 1][j]
        && lcs[i - 1][j - 1] >= lcs[i][j - 1]
      {
        ops.push(PatchOp::Update(i - 1, j - 1));
        i -= 1;
        j -= 1;
      } else if lcs[i][j - 1] >= lcs[i - 1][j] {
        ops.push(PatchOp::Insert(j - 1));
        j -= 1;
      } else {
        ops.push(PatchOp::Delete(i - 1));
        i -= 1;
      }
    } else if j > 0 {
      ops.push(PatchOp::Insert(j - 1));
      j -= 1;
    } else {
      ops.push(PatchOp::Delete(i - 1));
      i -= 1;
    }
  }

  ops.reverse();
  ops
}

fn update_block_props(
  doc: &Doc,
  blocks_map: &mut Map,
  node: &StoredNode,
  target: &BlockSpec,
  preserve_text: bool,
) -> Result<(), ParseError> {
  let Some(mut block) = blocks_map.get(&node.id).and_then(|v| v.to_map()) else {
    return Err(ParseError::ParserError(format!("Block {} not found", node.id)));
  };

  let preserve = match target.flavour {
    BlockFlavour::Image
    | BlockFlavour::Table
    | BlockFlavour::Bookmark
    | BlockFlavour::EmbedYoutube
    | BlockFlavour::EmbedIframe => preserve_text,
    _ => preserve_text || text_delta_eq(&node.spec.text, &target.text),
  };

  apply_block_spec(
    doc,
    &mut block,
    target,
    ApplyBlockOptions {
      preserve_text: preserve,
      clear_missing: true,
    },
  )?;

  Ok(())
}

fn sync_children(doc: &Doc, blocks_map: &mut Map, block_id: &str, children: &[String]) -> Result<(), ParseError> {
  let Some(mut block) = blocks_map.get(block_id).and_then(|v| v.to_map()) else {
    return Err(ParseError::ParserError("Block not found".into()));
  };

  let current_children = collect_child_ids(&block);
  if current_children != children {
    insert_children(doc, &mut block, children)?;
  }

  Ok(())
}

fn collect_tree_ids(node: &StoredNode, output: &mut Vec<String>) {
  output.push(node.id.clone());
  for child in &node.children {
    collect_tree_ids(child, output);
  }
}

fn check_limits(current: &[StoredNode], target: &[BlockNode]) -> Result<(), ParseError> {
  let current_count = count_tree_nodes(current);
  let target_count = count_tree_nodes(target);

  if current_count > MAX_BLOCKS || target_count > MAX_BLOCKS {
    return Err(ParseError::ParserError("block_count_too_large".into()));
  }

  if current_count.saturating_mul(target_count) > MAX_LCS_CELLS {
    return Err(ParseError::ParserError("diff_matrix_too_large".into()));
  }

  Ok(())
}

#[cfg(test)]
#[path = "../tests/write/update.rs"]
mod tests;
