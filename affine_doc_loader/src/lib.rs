mod block_spec;
mod blocksuite;
mod doc_loader;
mod error;
mod markdown;
mod read;
#[cfg(test)]
#[path = "tests/roundtrip_tests.rs"]
mod roundtrip_tests;
mod schema;
mod table;
mod value;
mod workspace;
mod write;

use std::collections::BTreeSet;

use block_spec::{BlockFlavour, BlockNode};
pub use error::ParseError;
pub use read::{
  BlobRef, BlockInfo, Bounds, CanvasBlock, CanvasElement, CanvasProjectionV1, CrawlResult, DocumentSearchProjectionV1,
  DocumentSearchUnit, MarkdownResult, PageDocContent, ProjectionWarning, SearchUnitSource, Visibility,
  WorkspaceDocContent, WorkspaceRootProjection, get_blob_refs_from_binary, get_doc_ids_from_binary,
  parse_doc_from_binary, parse_doc_to_markdown, parse_page_doc, parse_workspace_doc, project_canvas,
  project_document_search, project_workspace_root,
};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
pub use workspace::{
  CollectionFact, CollectionFilter, DocumentFacts, TagFact, WorkspaceFacts, WorkspaceProjectionError, apply_favorites,
  apply_workspace_db, evaluate_collection, project_orm_records, project_workspace_root_facts,
};
pub use write::{
  add_doc_to_root_doc, build_full_doc, build_public_root_doc, update_doc, update_doc_properties, update_doc_title,
  update_root_doc_meta_title,
};
use y_octo::{TextAttributes, TextDeltaOp, TextInsert};

pub fn build_doc_snapshot(title: &str, markdown: &str, doc_id: &str) -> Result<serde_json::Value, ParseError> {
  build_doc_snapshot_inner(title, markdown::parse_markdown_blocks(markdown)?, doc_id)
}

pub fn build_doc_snapshot_with_id_hints(
  title: &str,
  markdown: &str,
  doc_id: &str,
  import_namespace: &str,
  hint_token: &str,
) -> Result<serde_json::Value, ParseError> {
  if import_namespace.is_empty()
    || import_namespace.len() > 64
    || !import_namespace
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
  {
    return Err(ParseError::ParserError("invalid_import_namespace".into()));
  }
  if hint_token.is_empty()
    || hint_token.len() > 64
    || !hint_token
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
  {
    return Err(ParseError::ParserError("invalid_block_id_hint_token".into()));
  }
  let blocks = markdown::parse_markdown_blocks_with_id_hints(markdown, hint_token)?;
  validate_snapshot_block_ids(&blocks, doc_id, import_namespace)?;
  build_doc_snapshot_inner(title, blocks, doc_id)
}

fn build_doc_snapshot_inner(
  title: &str,
  blocks: Vec<BlockNode>,
  doc_id: &str,
) -> Result<serde_json::Value, ParseError> {
  Ok(json!({
    "type": "page",
    "meta": {
      "id": doc_id,
      "title": title,
      "createDate": 0,
      "tags": [],
    },
    "blocks": {
      "type": "block",
      "id": format!("block:{doc_id}"),
      "flavour": "affine:page",
      "props": {
        "title": {
          "$blocksuite:internal:text$": true,
          "delta": [{ "insert": title }]
        }
      },
      "children": [{
        "type": "block",
        "id": format!("block:{doc_id}:note"),
        "flavour": "affine:note",
        "props": {
          "xywh": "[0,0,800,95]",
          "background": {
            "light": "#ffffff",
            "dark": "#252525"
          },
          "index": "a0",
          "hidden": false,
          "displayMode": "both"
        },
        "children": snapshot_blocks(&blocks, doc_id)
      }]
    }
  }))
}

fn validate_snapshot_block_ids(blocks: &[BlockNode], doc_id: &str, import_namespace: &str) -> Result<(), ParseError> {
  fn visit(
    blocks: &[BlockNode],
    parent_id: &str,
    allowed_prefix: &str,
    ids: &mut BTreeSet<String>,
  ) -> Result<(), ParseError> {
    for (index, block) in blocks.iter().enumerate() {
      let fallback = format!("{parent_id}:{index}");
      let id = block.id.as_deref().unwrap_or(&fallback);
      if id.len() > 512
        || block.id.as_deref().is_some_and(|id| !id.starts_with(allowed_prefix))
        || !ids.insert(id.to_string())
      {
        return Err(ParseError::ParserError("invalid_or_duplicate_block_id_hint".into()));
      }
      visit(&block.children, id, allowed_prefix, ids)?;
    }
    Ok(())
  }

  let page_id = format!("block:{doc_id}");
  let note_id = format!("{page_id}:note");
  let mut ids = BTreeSet::from([page_id, note_id.clone()]);
  visit(
    blocks,
    &format!("block:{doc_id}"),
    &format!("block:{doc_id}:import:{import_namespace}:"),
    &mut ids,
  )
}

fn snapshot_blocks(blocks: &[BlockNode], doc_id: &str) -> Vec<JsonValue> {
  if blocks.is_empty() {
    return vec![json!({
      "type": "block",
      "id": format!("block:{doc_id}:empty"),
      "flavour": "affine:paragraph",
      "props": {
        "type": "text",
        "text": text_value(&[])
      },
      "children": []
    })];
  }

  blocks
    .iter()
    .enumerate()
    .map(|(index, block)| snapshot_block(block, &format!("block:{doc_id}:{index}")))
    .collect()
}

fn snapshot_block(block: &BlockNode, id: &str) -> JsonValue {
  let id = block.id.as_deref().unwrap_or(id);
  json!({
    "type": "block",
    "id": id,
    "flavour": block.spec.flavour.as_str(),
    "props": block_props(block, id),
    "children": block.children.iter().enumerate().map(|(index, child)| {
      snapshot_block(child, &format!("{id}:{index}"))
    }).collect::<Vec<_>>()
  })
}

fn block_props(block: &BlockNode, id: &str) -> JsonValue {
  match block.spec.flavour {
    BlockFlavour::Image => block
      .spec
      .image
      .as_ref()
      .map(|image| {
        let mut props = JsonMap::new();
        props.insert("sourceId".into(), JsonValue::String(image.source_id.clone()));
        if let Some(caption) = image.caption.as_ref() {
          props.insert("caption".into(), JsonValue::String(caption.clone()));
        }
        if let Some(width) = image.width {
          props.insert("width".into(), json!(width));
        }
        if let Some(height) = image.height {
          props.insert("height".into(), json!(height));
        }
        JsonValue::Object(props)
      })
      .unwrap_or_else(|| json!({})),
    BlockFlavour::Table => block
      .spec
      .table
      .as_ref()
      .map(|table| table_props(&table.rows, id))
      .unwrap_or_else(|| json!({})),
    BlockFlavour::Bookmark => block
      .spec
      .bookmark
      .as_ref()
      .map(|bookmark| {
        let mut props = JsonMap::new();
        props.insert("url".into(), JsonValue::String(bookmark.url.clone()));
        if let Some(caption) = bookmark.caption.as_ref() {
          props.insert("caption".into(), JsonValue::String(caption.clone()));
        }
        JsonValue::Object(props)
      })
      .unwrap_or_else(|| json!({})),
    BlockFlavour::EmbedYoutube => block
      .spec
      .embed_youtube
      .as_ref()
      .map(|embed| json!({ "videoId": embed.video_id }))
      .unwrap_or_else(|| json!({})),
    BlockFlavour::EmbedIframe => block
      .spec
      .embed_iframe
      .as_ref()
      .map(|embed| json!({ "url": embed.url }))
      .unwrap_or_else(|| json!({})),
    BlockFlavour::Callout => json!({ "emoji": block.spec.callout_emoji.as_deref().unwrap_or("💡") }),
    _ => {
      let mut props = JsonMap::new();
      if let Some(block_type) = block.spec.block_type_str() {
        props.insert("type".into(), JsonValue::String(block_type.to_string()));
      }
      if !block.spec.text.is_empty() || block.spec.flavour != BlockFlavour::Divider {
        props.insert("text".into(), text_value(&block.spec.text));
      }
      if let Some(checked) = block.spec.checked {
        props.insert("checked".into(), JsonValue::Bool(checked));
      }
      if let Some(language) = block.spec.language.as_ref() {
        props.insert("language".into(), JsonValue::String(language.clone()));
      }
      if let Some(order) = block.spec.order {
        props.insert("order".into(), json!(order));
      }
      JsonValue::Object(props)
    }
  }
}

fn table_props(rows: &[Vec<String>], id: &str) -> JsonValue {
  let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
  let mut columns = JsonMap::new();
  let mut row_values = JsonMap::new();
  let mut cells = JsonMap::new();
  let column_ids = (0..column_count)
    .map(|index| {
      let column_id = format!("{id}:column:{index}");
      columns.insert(
        column_id.clone(),
        json!({
          "columnId": column_id,
          "order": schema::table_order_key(index)
        }),
      );
      column_id
    })
    .collect::<Vec<_>>();

  for (row_index, row) in rows.iter().enumerate() {
    let row_id = format!("{id}:row:{row_index}");
    row_values.insert(
      row_id.clone(),
      json!({
        "rowId": row_id,
        "order": schema::table_order_key(row_index)
      }),
    );
    for (column_index, column_id) in column_ids.iter().enumerate() {
      let cell_id = format!("{row_id}:{column_id}");
      cells.insert(
        cell_id,
        json!({
          "text": text_value(&markdown::table_cell_ops(row.get(column_index).map(String::as_str).unwrap_or("")))
        }),
      );
    }
  }

  json!({
    "columns": columns,
    "rows": row_values,
    "cells": cells
  })
}

fn text_value(delta: &[TextDeltaOp]) -> JsonValue {
  json!({
    "$blocksuite:internal:text$": true,
    "delta": delta.iter().filter_map(delta_op).collect::<Vec<_>>()
  })
}

fn delta_op(op: &TextDeltaOp) -> Option<JsonValue> {
  let TextDeltaOp::Insert { insert, format } = op else {
    return None;
  };
  let TextInsert::Text(text) = insert else {
    return None;
  };
  let mut value = JsonMap::new();
  value.insert("insert".into(), JsonValue::String(text.clone()));
  if let Some(format) = format.as_ref().and_then(attributes_value) {
    value.insert("attributes".into(), format);
  }
  Some(JsonValue::Object(value))
}

fn attributes_value(attributes: &TextAttributes) -> Option<JsonValue> {
  if attributes.is_empty() {
    return None;
  }
  let mut values = JsonMap::new();
  for (key, value) in attributes {
    if let Ok(value) = serde_json::to_value(value) {
      values.insert(key.clone(), value);
    }
  }
  Some(JsonValue::Object(values))
}

#[cfg(test)]
#[path = "tests/id_hint_tests.rs"]
mod id_hint_tests;
