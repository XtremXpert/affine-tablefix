use serde::{Deserialize, Serialize};

use super::{
  DocContext, MapRead, PAGE_FLAVOUR, ParseError, SummaryBuilder, ValueRead, database_summary_text, get_block_id,
  get_flavour, get_string, load_doc, load_read_doc, table_cell_texts, text_content_for_summary,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDocContent {
  pub title: String,
  pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDocContent {
  pub name: String,
  #[serde(rename = "avatarKey")]
  pub avatar_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobRef {
  pub blob_key: String,
  pub block_id: String,
  pub flavour: String,
}

pub fn get_blob_refs_from_binary(doc_bin: Vec<u8>) -> Result<Vec<BlobRef>, ParseError> {
  let doc = load_read_doc(doc_bin)?;
  let Some(blocks) = doc.map("blocks") else {
    return Ok(Vec::new());
  };
  Ok(get_blob_refs(&blocks))
}

fn get_blob_refs(blocks: &impl MapRead) -> Vec<BlobRef> {
  blocks
    .values()
    .filter_map(|value| value.as_map_view())
    .filter_map(|block| {
      let flavour = get_flavour(&block)?;
      if !matches!(flavour.as_str(), "affine:attachment" | "affine:image") {
        return None;
      }
      Some(BlobRef {
        blob_key: get_string(&block, "prop:sourceId")?,
        block_id: get_block_id(&block)?,
        flavour,
      })
    })
    .collect()
}

pub fn parse_workspace_doc(doc_bin: Vec<u8>) -> Result<Option<WorkspaceDocContent>, ParseError> {
  let doc = load_doc(&doc_bin, None)?;
  let meta = match doc.get_map("meta") {
    Ok(meta) => meta,
    Err(_) => return Ok(None),
  };
  let name = get_string(&meta, "name").unwrap_or_default();
  let avatar_key = get_string(&meta, "avatar").unwrap_or_default();
  Ok(Some(WorkspaceDocContent { name, avatar_key }))
}

pub fn parse_page_doc(
  doc_bin: Vec<u8>,
  max_summary_length: Option<isize>,
) -> Result<Option<PageDocContent>, ParseError> {
  let doc = load_read_doc(doc_bin)?;
  let blocks_map = match doc.map("blocks") {
    Some(map) => map,
    None => return Ok(None),
  };
  Ok(parse_page_blocks(&blocks_map, max_summary_length))
}

pub(super) fn parse_page_blocks<M: MapRead>(
  blocks_map: &M,
  max_summary_length: Option<isize>,
) -> Option<PageDocContent> {
  if blocks_map.is_empty() {
    return None;
  }
  let context = DocContext::from_blocks_map(blocks_map, PAGE_FLAVOUR)?;
  let mut walker = context.walker();
  let mut content = PageDocContent {
    title: context
      .block_pool
      .get(&context.root_block_id)
      .and_then(|block| get_string(block, "prop:title"))
      .unwrap_or_default(),
    summary: String::new(),
  };
  let mut summary = SummaryBuilder::new(max_summary_length.unwrap_or(150));
  while let Some((_parent_block_id, block_id)) = walker.next() {
    let Some(block) = context.block_pool.get(&block_id) else {
      continue;
    };
    let Some(flavour) = get_flavour(block) else {
      continue;
    };
    match flavour.as_str() {
      "affine:page" | "affine:note" => walker.enqueue_children(&block_id, block),
      "affine:attachment" | "affine:transcription" | "affine:callout" => {
        if summary.is_unlimited() {
          walker.enqueue_children(&block_id, block);
        }
      }
      "affine:database" => {
        if summary.is_unlimited()
          && let Some(text) = database_summary_text(block, &context)
        {
          summary.push_raw(&text);
        }
      }
      "affine:table" => {
        if summary.is_unlimited() {
          let contents = table_cell_texts(block);
          if !contents.is_empty() {
            summary.push_raw(&contents.join("|"));
          }
        }
      }
      "affine:paragraph" | "affine:list" | "affine:code" => {
        walker.enqueue_children(&block_id, block);
        if let Some((text, len)) = text_content_for_summary(block, "prop:text") {
          summary.push_text(&text, len);
        }
      }
      _ => {}
    }
  }
  content.summary = summary.into_string();
  Some(content)
}
