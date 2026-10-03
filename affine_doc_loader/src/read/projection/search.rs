use std::collections::HashSet;

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
  DocumentSearchProjectionV1, DocumentSearchUnit, PROJECTION_VERSION, ParseError, ProjectionTarget, SearchUnitSource,
  Visibility, normalize_document,
};

pub fn project_document_search(
  doc_bin: Vec<u8>,
  doc_id: String,
  revision: String,
) -> Result<DocumentSearchProjectionV1, ParseError> {
  let normalized = normalize_document(doc_bin, ProjectionTarget::Search)?;
  let mut units = Vec::new();
  if !normalized.title.trim().is_empty() {
    units.push(DocumentSearchUnit {
      unit_id: format!("block:{}", normalized.page_root_id),
      source: SearchUnitSource::PageBlock,
      visibility: Visibility::Page,
      block_id: Some(normalized.page_root_id.clone()),
      element_id: None,
      frame_id: None,
      blob_id: None,
      ref_doc_ids: Vec::new(),
      refs: Vec::new(),
      parent_flavour: None,
      parent_block_id: None,
      additional: None,
      unit_type: "page".into(),
      text: normalized.title.clone(),
    });
  }
  for block in &normalized.search_blocks {
    if matches!(block.block_type.as_str(), "note" | "edgeless-text") && block.has_children {
      continue;
    }
    units.push(DocumentSearchUnit {
      unit_id: format!("block:{}", block.id),
      source: if block.visibility == Visibility::Page {
        SearchUnitSource::PageBlock
      } else {
        SearchUnitSource::CanvasBlock
      },
      visibility: block.visibility,
      block_id: Some(block.id.clone()),
      element_id: None,
      frame_id: None,
      blob_id: block.blob_id.clone(),
      ref_doc_ids: block.ref_doc_ids.clone(),
      refs: block.refs.clone(),
      parent_flavour: block.parent_flavour.clone(),
      parent_block_id: block.parent_block_id.clone(),
      additional: block.additional.clone(),
      unit_type: block.block_type.clone(),
      text: block.text.clone(),
    });
  }
  let mindmap_nodes = normalized
    .elements
    .iter()
    .filter(|element| element.element_type == "mindmap")
    .flat_map(|element| element.child_ids.iter().cloned())
    .collect::<HashSet<_>>();
  for element in &normalized.elements {
    let text = element
      .text
      .as_deref()
      .or(element.title.as_deref())
      .map(str::trim)
      .filter(|text| !text.is_empty());
    if let Some(text) = text {
      units.push(DocumentSearchUnit {
        unit_id: format!("element:{}", element.id),
        source: SearchUnitSource::SurfaceElement,
        visibility: Visibility::Edgeless,
        block_id: None,
        element_id: Some(element.id.clone()),
        frame_id: element.frame_id.clone(),
        blob_id: None,
        ref_doc_ids: Vec::new(),
        refs: Vec::new(),
        parent_flavour: None,
        parent_block_id: None,
        additional: None,
        unit_type: if mindmap_nodes.contains(&element.id) {
          "mindmap-node".into()
        } else {
          element.element_type.clone()
        },
        text: text.to_string(),
      });
    }
  }
  units.sort_by(|left, right| left.unit_id.cmp(&right.unit_id));
  units.dedup_by(|left, right| left.unit_id == right.unit_id);
  let source_hash = canonical_hash(&(PROJECTION_VERSION, &doc_id, &revision, &normalized.title, &units));
  Ok(DocumentSearchProjectionV1 {
    version: PROJECTION_VERSION,
    doc_id,
    revision,
    source_hash,
    title: normalized.title,
    units,
    warnings: normalized.warnings,
  })
}

fn canonical_hash(value: &impl Serialize) -> String {
  let bytes = serde_json::to_vec(value).expect("projection serialization is infallible");
  Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
