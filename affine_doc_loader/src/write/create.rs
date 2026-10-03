//! Markdown to YDoc conversion module
//!
//! Converts markdown content into AFFiNE-compatible y-octo document binary
//! format.

use y_octo::DocOptions;

use super::{
  super::{
    markdown::parse_markdown_blocks,
    schema::{PROP_BACKGROUND, PROP_DISPLAY_MODE, PROP_ELEMENTS, PROP_HIDDEN, PROP_INDEX, PROP_XYWH, SURFACE_FLAVOUR},
  },
  builder::{
    BOXED_NATIVE_TYPE, NOTE_BG_DARK, NOTE_BG_LIGHT, boxed_empty_map, insert_block_map, insert_block_tree,
    insert_children, insert_sys_fields, insert_text, note_background_map, text_ops_from_plain,
  },
  *,
};

/// Converts markdown into an AFFiNE-compatible y-octo document binary.
///
/// # Arguments
/// * `title` - The document title
/// * `markdown` - The markdown content to convert
/// * `doc_id` - The document ID to use
///
/// # Returns
/// A binary vector containing the y-octo encoded document update
pub fn build_full_doc(title: &str, markdown: &str, doc_id: &str) -> Result<Vec<u8>, ParseError> {
  let nodes = parse_markdown_blocks(markdown)?;
  build_doc_update(doc_id, title, &nodes)
}

fn build_doc_update(doc_id: &str, title: &str, blocks: &[BlockNode]) -> Result<Vec<u8>, ParseError> {
  let doc = DocOptions::new().with_guid(doc_id.to_string()).build();
  let mut blocks_map = doc.get_or_create_map("blocks")?;

  let page_id = nanoid::nanoid!();
  let surface_id = nanoid::nanoid!();
  let note_id = nanoid::nanoid!();

  // Insert root blocks first to establish stable IDs.
  let mut page_map = insert_block_map(&doc, &mut blocks_map, &page_id)?;
  let mut surface_map = insert_block_map(&doc, &mut blocks_map, &surface_id)?;
  let mut note_map = insert_block_map(&doc, &mut blocks_map, &note_id)?;

  // Create content blocks under note.
  let content_ids = insert_block_trees(&doc, &mut blocks_map, blocks)?;

  // Page block
  insert_sys_fields(&mut page_map, &page_id, PAGE_FLAVOUR)?;
  insert_children(&doc, &mut page_map, &[surface_id.clone(), note_id.clone()])?;
  insert_text(&doc, &mut page_map, PROP_TITLE, &text_ops_from_plain(title))?;

  // Surface block
  insert_sys_fields(&mut surface_map, &surface_id, SURFACE_FLAVOUR)?;
  insert_children(&doc, &mut surface_map, &[])?;
  let mut boxed = boxed_empty_map(&doc)?;
  surface_map.insert(PROP_ELEMENTS.to_string(), Value::Map(boxed.clone()))?;
  boxed.insert("type".to_string(), Any::String(BOXED_NATIVE_TYPE.to_string()))?;
  let value = doc.create_map()?;
  boxed.insert("value".to_string(), Value::Map(value))?;

  // Note block
  insert_sys_fields(&mut note_map, &note_id, NOTE_FLAVOUR)?;
  insert_children(&doc, &mut note_map, &content_ids)?;
  let mut background = note_background_map(&doc)?;
  note_map.insert(PROP_BACKGROUND.to_string(), Value::Map(background.clone()))?;
  background.insert("light".to_string(), Any::String(NOTE_BG_LIGHT.to_string()))?;
  background.insert("dark".to_string(), Any::String(NOTE_BG_DARK.to_string()))?;
  note_map.insert(PROP_XYWH.to_string(), Any::String("[0,0,800,95]".to_string()))?;
  note_map.insert(PROP_INDEX.to_string(), Any::String("a0".to_string()))?;
  note_map.insert(PROP_HIDDEN.to_string(), Any::False)?;
  note_map.insert(PROP_DISPLAY_MODE.to_string(), Any::String("both".to_string()))?;

  Ok(doc.encode_update_v1()?)
}

fn insert_block_trees(doc: &Doc, blocks_map: &mut Map, blocks: &[BlockNode]) -> Result<Vec<String>, ParseError> {
  let mut ids = Vec::with_capacity(blocks.len());
  for block in blocks {
    let id = insert_block_tree(doc, blocks_map, block)?;
    ids.push(id);
  }
  Ok(ids)
}

#[cfg(test)]
#[path = "../tests/write/create.rs"]
mod tests;
