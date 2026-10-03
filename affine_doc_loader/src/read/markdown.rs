use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
  BlockFlavour, BlockSpec, DEFAULT_PAGE_TITLE, DeltaToMdOptions, DocContext, MapRead, MarkdownRenderer, MarkdownWriter,
  PAGE_FLAVOUR, ParseError, build_database_table, database_table_markdown, get_flavour, get_list_depth, get_string,
  load_read_doc,
};

const KNOWN_UNSUPPORTED_FLAVOURS: [&str; 10] = [
  "affine:attachment",
  "affine:callout",
  "affine:note",
  "affine:edgeless-text",
  "affine:embed-linked-doc",
  "affine:embed-synced-doc",
  "affine:frame",
  "affine:latex",
  "affine:surface",
  "affine:surface-ref",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownResult {
  pub title: String,
  pub markdown: String,
  pub known_unsupported_blocks: Vec<String>,
  pub unknown_blocks: Vec<String>,
}

pub fn parse_doc_to_markdown(
  doc_bin: Vec<u8>,
  _doc_id: String,
  ai_editable: bool,
  doc_url_prefix: Option<String>,
) -> Result<MarkdownResult, ParseError> {
  let doc = load_read_doc(doc_bin)?;
  let blocks_map = doc
    .map("blocks")
    .ok_or_else(|| ParseError::ParserError("blocks map not found".into()))?;
  parse_blocks_to_markdown(&blocks_map, ai_editable, doc_url_prefix)
}

pub(super) fn parse_blocks_to_markdown<M: MapRead>(
  blocks_map: &M,
  ai_editable: bool,
  doc_url_prefix: Option<String>,
) -> Result<MarkdownResult, ParseError> {
  if blocks_map.is_empty() {
    return Ok(MarkdownResult {
      title: "".into(),
      markdown: "".into(),
      known_unsupported_blocks: vec![],
      unknown_blocks: vec![],
    });
  }
  let context = DocContext::from_blocks_map(blocks_map, PAGE_FLAVOUR)
    .ok_or_else(|| ParseError::ParserError("root block not found".into()))?;
  let root_block_id = context.root_block_id.clone();
  let mut walker = context.walker();
  let mut doc_title = String::from(DEFAULT_PAGE_TITLE);
  let mut markdown = String::new();
  let mut known_unsupported_blocks = Vec::new();
  let mut unknown_blocks = Vec::new();
  let mut skipped_subtrees = HashSet::new();
  let md_options = DeltaToMdOptions::new(doc_url_prefix);
  let renderer = MarkdownRenderer::new(&md_options);
  while let Some((_parent_block_id, block_id)) = walker.next() {
    let Some(block) = context.block_pool.get(&block_id) else {
      continue;
    };
    let Some(flavour) = get_flavour(block) else {
      continue;
    };
    if flavour == PAGE_FLAVOUR {
      walker.enqueue_children(&block_id, block);
      doc_title = get_string(block, "prop:title").unwrap_or_default();
      continue;
    }
    let parent_flavour = context
      .parent_lookup
      .get(&block_id)
      .and_then(|id| context.block_pool.get(id))
      .and_then(get_flavour);
    if parent_flavour.as_deref() == Some("affine:database") {
      continue;
    }
    walker.enqueue_children(&block_id, block);
    if is_known_unsupported_flavour(&flavour) {
      known_unsupported_blocks.push(format!("{block_id}:{flavour}"));
      if is_edgeless_flavour(&flavour) {
        skipped_subtrees.insert(block_id.clone());
      }
      continue;
    }
    if BlockFlavour::from_str(&flavour).is_none() && flavour != "affine:database" {
      unknown_blocks.push(format!("{block_id}:{flavour}"));
      skipped_subtrees.insert(block_id.clone());
      continue;
    }
    if has_skipped_ancestor(&block_id, &context.parent_lookup, &skipped_subtrees) {
      continue;
    }
    let ai_block = ai_editable && block_level(&block_id, &root_block_id, &context.parent_lookup) == 2;
    let mut block_markdown = String::new();
    if flavour == "affine:database" {
      let title = get_string(block, "prop:title").unwrap_or_default();
      block_markdown.push_str(&format!("\n### {title}\n"));
      if let Some(table) = build_database_table(block, &context, &md_options)
        && let Some(table_md) = database_table_markdown(table)
      {
        MarkdownWriter::new(&mut block_markdown).push_table(&table_md);
      }
    } else {
      let Some(block_flavour) = BlockFlavour::from_str(&flavour) else {
        continue;
      };
      let spec = BlockSpec::from_block_map_with_flavour(block, block_flavour);
      let list_depth = if block_flavour == BlockFlavour::List {
        get_list_depth(&block_id, &context.parent_lookup, &context.block_pool)
      } else {
        0
      };
      renderer.write_block(&mut block_markdown, &spec, list_depth);
    }
    if ai_block {
      markdown.push_str(&format!("<!-- block_id={block_id} flavour={flavour} -->\n"));
    }
    markdown.push_str(&block_markdown);
  }
  Ok(MarkdownResult {
    title: doc_title,
    markdown,
    known_unsupported_blocks,
    unknown_blocks,
  })
}

fn is_known_unsupported_flavour(flavour: &str) -> bool {
  KNOWN_UNSUPPORTED_FLAVOURS.contains(&flavour) || flavour.starts_with("affine:edgeless-")
}

fn is_edgeless_flavour(flavour: &str) -> bool {
  matches!(flavour, "affine:surface" | "affine:frame" | "affine:surface-ref") || flavour.starts_with("affine:edgeless-")
}

fn has_skipped_ancestor(
  block_id: &str,
  parent_lookup: &HashMap<String, String>,
  skipped_subtrees: &HashSet<String>,
) -> bool {
  let mut cursor = parent_lookup.get(block_id).cloned();
  while let Some(parent_id) = cursor {
    if skipped_subtrees.contains(&parent_id) {
      return true;
    }
    cursor = parent_lookup.get(&parent_id).cloned();
  }
  false
}

fn block_level(block_id: &str, root_id: &str, parent_lookup: &HashMap<String, String>) -> usize {
  let mut level = 0;
  let mut cursor = block_id;
  while let Some(parent) = parent_lookup.get(cursor) {
    level += 1;
    if parent == root_id {
      break;
    }
    cursor = parent;
  }
  level
}
