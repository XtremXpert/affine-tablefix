use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::{
  BOOKMARK_FLAVOURS, DEFAULT_PAGE_TITLE, DocContext, ImageSpec, MapRead, NOTE_FLAVOUR, PAGE_FLAVOUR, ParseError,
  SummaryBuilder, TextRead, ValueRead, build_reference_payload, collect_database_cell_references,
  extract_inline_references, gather_database_texts, get_block_id, get_flavour, get_string, load_read_doc,
  nearest_by_flavour, params_value_to_json, table_cell_texts, value_to_string,
};

const SUMMARY_LIMIT: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
  pub block_id: String,
  pub flavour: String,
  pub content: Option<Vec<String>>,
  pub blob: Option<Vec<String>>,
  pub ref_doc_id: Option<Vec<String>>,
  pub ref_info: Option<Vec<String>>,
  pub parent_flavour: Option<String>,
  pub parent_block_id: Option<String>,
  pub additional: Option<String>,
}

impl BlockInfo {
  fn base(
    block_id: &str,
    flavour: &str,
    parent_flavour: Option<&String>,
    parent_block_id: Option<&String>,
    additional: Option<String>,
  ) -> Self {
    Self {
      block_id: block_id.to_string(),
      flavour: flavour.to_string(),
      content: None,
      blob: None,
      ref_doc_id: None,
      ref_info: None,
      parent_flavour: parent_flavour.cloned(),
      parent_block_id: parent_block_id.cloned(),
      additional,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlResult {
  pub blocks: Vec<BlockInfo>,
  pub title: String,
  pub summary: String,
}

pub fn parse_doc_from_binary(doc_bin: Vec<u8>, _doc_id: String) -> Result<CrawlResult, ParseError> {
  let doc = load_read_doc(doc_bin)?;
  let blocks_map = doc
    .map("blocks")
    .ok_or_else(|| ParseError::ParserError("blocks map not found".into()))?;
  crawl_blocks(&blocks_map)
}

pub(super) fn crawl_blocks<M: MapRead>(blocks_map: &M) -> Result<CrawlResult, ParseError> {
  if blocks_map.is_empty() {
    return Err(ParseError::ParserError("blocks map is empty".into()));
  }
  let context = DocContext::from_blocks_map(blocks_map, PAGE_FLAVOUR)
    .ok_or_else(|| ParseError::ParserError("root block not found".into()))?;
  let mut walker = context.walker();
  let mut blocks = Vec::with_capacity(context.block_pool.len());
  let mut doc_title = String::new();
  let mut summary = SummaryBuilder::new(SUMMARY_LIMIT as isize);
  while let Some((parent_block_id, block_id)) = walker.next() {
    let Some(block) = context.block_pool.get(&block_id) else {
      continue;
    };
    let Some(flavour) = get_flavour(block) else {
      continue;
    };
    let parent_block = parent_block_id.as_ref().and_then(|id| context.block_pool.get(id));
    let parent_flavour = parent_block.and_then(get_flavour);
    let note_block = nearest_by_flavour(&block_id, NOTE_FLAVOUR, &context.parent_lookup, &context.block_pool);
    let note_block_id = note_block.as_ref().and_then(get_block_id);
    let display_mode = determine_display_mode(note_block.as_ref());
    walker.enqueue_children(&block_id, block);
    let build_block = |database_name: Option<&String>| {
      BlockInfo::base(
        &block_id,
        &flavour,
        parent_flavour.as_ref(),
        parent_block_id.as_ref(),
        compose_additional(&display_mode, note_block_id.as_ref(), database_name),
      )
    };
    if flavour == PAGE_FLAVOUR {
      let title = get_string(block, "prop:title").unwrap_or_default();
      doc_title = title.clone();
      let mut info = build_block(None);
      info.content = Some(vec![title]);
      blocks.push(info);
      continue;
    }
    if matches!(flavour.as_str(), "affine:paragraph" | "affine:list" | "affine:code") {
      if let Some(text) = block.get_value("prop:text").and_then(|value| value.as_text_view()) {
        let database_name = if flavour == "affine:paragraph" && parent_flavour.as_deref() == Some("affine:database") {
          parent_block.and_then(|map| get_string(map, "prop:title"))
        } else {
          None
        };
        let content = text.text();
        let text_len = content.encode_utf16().count();
        let refs = extract_inline_references(&text.delta());
        let mut info = build_block(database_name.as_ref());
        info.content = Some(vec![content.clone()]);
        if !refs.is_empty() {
          info.ref_doc_id = Some(refs.iter().map(|reference| reference.doc_id.clone()).collect());
          info.ref_info = Some(refs.into_iter().map(|reference| reference.payload).collect());
        }
        blocks.push(info);
        summary.push_text(&content, text_len);
      }
      continue;
    }
    if matches!(flavour.as_str(), "affine:embed-linked-doc" | "affine:embed-synced-doc") {
      if let Some(page_id) = get_string(block, "prop:pageId") {
        let mut info = build_block(None);
        let payload = embed_ref_payload(block, &page_id);
        apply_doc_ref(&mut info, page_id, payload);
        blocks.push(info);
      }
      continue;
    }
    if flavour == "affine:attachment" {
      if let Some(blob_id) = get_string(block, "prop:sourceId") {
        let mut info = build_block(None);
        apply_blob_info(&mut info, blob_id, get_string(block, "prop:name").unwrap_or_default());
        blocks.push(info);
      }
      continue;
    }
    if flavour == "affine:image" {
      let image = ImageSpec::from_block_map(block);
      if !image.source_id.is_empty() {
        let mut info = build_block(None);
        apply_blob_info(&mut info, image.source_id, image.caption.unwrap_or_default());
        blocks.push(info);
      }
      continue;
    }
    if flavour == "affine:surface" {
      let mut info = build_block(None);
      info.content = Some(gather_surface_texts(block));
      blocks.push(info);
      continue;
    }
    if flavour == "affine:database" {
      let (texts, database_name) = gather_database_texts(block);
      let mut info = BlockInfo::base(
        &block_id,
        &flavour,
        parent_flavour.as_ref(),
        parent_block_id.as_ref(),
        compose_additional(&display_mode, note_block_id.as_ref(), database_name.as_ref()),
      );
      info.content = Some(texts);
      let refs = collect_database_cell_references(block);
      if !refs.is_empty() {
        info.ref_doc_id = Some(refs.iter().map(|reference| reference.doc_id.clone()).collect());
        info.ref_info = Some(refs.into_iter().map(|reference| reference.payload).collect());
      }
      blocks.push(info);
      continue;
    }
    if flavour == "affine:latex" {
      if let Some(content) = get_string(block, "prop:latex") {
        let mut info = build_block(None);
        info.content = Some(vec![content]);
        blocks.push(info);
      }
      continue;
    }
    if flavour == "affine:table" {
      let mut info = build_block(None);
      info.content = Some(table_cell_texts(block));
      blocks.push(info);
      continue;
    }
    if BOOKMARK_FLAVOURS.contains(&flavour.as_str()) {
      blocks.push(build_block(None));
    }
  }
  if doc_title.is_empty() {
    doc_title = DEFAULT_PAGE_TITLE.into();
  }
  Ok(CrawlResult {
    blocks,
    title: doc_title,
    summary: summary.into_string(),
  })
}

pub(super) fn determine_display_mode<M: MapRead>(note_block: Option<&M>) -> String {
  match note_block.and_then(|block| get_string(block, "prop:displayMode")) {
    Some(mode) if mode == "both" => "page".into(),
    Some(mode) => mode,
    None => "edgeless".into(),
  }
}

pub(super) fn compose_additional(
  display_mode: &str,
  note_block_id: Option<&String>,
  database_name: Option<&String>,
) -> Option<String> {
  let mut payload = JsonMap::new();
  payload.insert("displayMode".into(), JsonValue::String(display_mode.to_string()));
  if let Some(note_id) = note_block_id {
    payload.insert("noteBlockId".into(), JsonValue::String(note_id.clone()));
  }
  if let Some(name) = database_name {
    payload.insert("databaseName".into(), JsonValue::String(name.clone()));
  }
  Some(JsonValue::Object(payload).to_string())
}

pub(super) fn embed_ref_payload(block: &impl MapRead, page_id: &str) -> Option<String> {
  let params = block.get_value("prop:params").as_ref().and_then(params_value_to_json);
  Some(build_reference_payload(page_id, params))
}

fn apply_blob_info(info: &mut BlockInfo, blob_id: String, content: String) {
  info.blob = Some(vec![blob_id]);
  info.content = Some(vec![content]);
}

fn apply_doc_ref(info: &mut BlockInfo, page_id: String, payload: Option<String>) {
  info.ref_doc_id = Some(vec![page_id]);
  if let Some(payload) = payload {
    info.ref_info = Some(vec![payload]);
  }
}

fn gather_surface_texts(block: &impl MapRead) -> Vec<String> {
  let mut texts = Vec::new();
  let Some(elements) = block.get_value("prop:elements").and_then(|value| value.as_map_view()) else {
    return texts;
  };
  if elements
    .get_value("type")
    .and_then(|value| value_to_string(&value))
    .as_deref()
    != Some("$blocksuite:internal:native$")
  {
    return texts;
  }
  if let Some(value_map) = elements.get_value("value").and_then(|value| value.as_map_view()) {
    for value in value_map.values() {
      if let Some(element) = value.as_map_view()
        && let Some(text) = element.get_value("text").and_then(|value| value.as_text_view())
      {
        texts.push(text.text());
      }
    }
  }
  texts.sort();
  texts
}
