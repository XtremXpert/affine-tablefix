mod block;
mod database;
mod document;
mod markdown;
mod projection;

pub use block::{BlockInfo, CrawlResult, parse_doc_from_binary};
pub use document::{
  BlobRef, PageDocContent, WorkspaceDocContent, get_blob_refs_from_binary, parse_page_doc, parse_workspace_doc,
};
pub use markdown::{MarkdownResult, parse_doc_to_markdown};
pub use projection::{
  Bounds, CanvasBlock, CanvasElement, CanvasProjectionV1, DocumentSearchProjectionV1, DocumentSearchUnit,
  ProjectionWarning, SearchUnitSource, Visibility, WorkspaceRootProjection, get_doc_ids_from_binary, project_canvas,
  project_document_search, project_workspace_root,
};

#[cfg(test)]
use self::{block::crawl_blocks, document::parse_page_blocks, markdown::parse_blocks_to_markdown};
use self::{
  block::{compose_additional, determine_display_mode, embed_ref_payload},
  database::{
    build_database_table, collect_database_cell_references, database_summary_text, database_table_markdown,
    gather_database_texts,
  },
};
use super::{
  ParseError,
  block_spec::{BlockFlavour, BlockSpec, ImageSpec},
  blocksuite::{
    DocContext, build_block_index, collect_child_ids, get_block_id, get_flavour, get_list_depth, get_string,
    nearest_by_flavour,
  },
  doc_loader::{load_doc, load_read_doc},
  markdown::{
    DeltaToMdOptions, InlineReferencePayload, MarkdownRenderer, MarkdownWriter, delta_value_to_inline_markdown,
    extract_inline_references, extract_inline_references_from_value, text_to_inline_markdown,
  },
  schema::{NOTE_FLAVOUR, PAGE_FLAVOUR},
  table::{MarkdownTableOptions, render_markdown_table},
  value::{
    AnyRead, ArrayRead, MapRead, TextRead, ValueRead, any_as_string, any_truthy, build_reference_payload,
    params_value_to_json, value_to_string,
  },
};

const DEFAULT_PAGE_TITLE: &str = "Untitled";
const BOOKMARK_FLAVOURS: [&str; 6] = [
  "affine:bookmark",
  "affine:embed-youtube",
  "affine:embed-iframe",
  "affine:embed-figma",
  "affine:embed-github",
  "affine:embed-loom",
];

struct SummaryBuilder {
  summary: String,
  remaining: Option<isize>,
}

impl SummaryBuilder {
  fn new(limit: isize) -> Self {
    let remaining = if limit < 0 { None } else { Some(limit) };
    Self {
      summary: String::new(),
      remaining,
    }
  }

  fn is_unlimited(&self) -> bool {
    self.remaining.is_none()
  }

  fn push_text(&mut self, text: &str, len: usize) {
    match self.remaining {
      None => self.summary.push_str(text),
      Some(remaining) if remaining > 0 => {
        self.summary.push_str(text);
        self.remaining = Some(remaining - len as isize);
      }
      _ => {}
    }
  }

  fn push_raw(&mut self, text: &str) {
    self.summary.push_str(text);
  }

  fn into_string(self) -> String {
    self.summary
  }
}

pub(super) fn text_content(block: &impl MapRead, key: &str) -> Option<(String, usize)> {
  block.get_value(key).and_then(|value| {
    value.as_text_view().map(|text| {
      let content = text.text();
      let len = content.encode_utf16().count();
      (content, len)
    })
  })
}

pub(super) fn text_content_for_summary(block: &impl MapRead, key: &str) -> Option<(String, usize)> {
  if let Some((text, len)) = text_content(block, key) {
    return Some((text, len));
  }
  block.get_value(key).and_then(|value| {
    value_to_string(&value).map(|text| {
      let len = text.chars().count();
      (text, len)
    })
  })
}

pub(super) fn table_cell_texts(block: &impl MapRead) -> Vec<String> {
  let mut contents = Vec::new();
  for key in block.keys() {
    if key.starts_with("prop:cells.")
      && key.ends_with(".text")
      && let Some(value) = block.get_value(key).and_then(|value| value_to_string(&value))
      && !value.is_empty()
    {
      contents.push(value);
    }
  }
  contents
}

#[cfg(test)]
#[path = "../tests/read/tests.rs"]
mod tests;
