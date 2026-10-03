use std::collections::BTreeMap;

use super::{
  Bounds, CanvasProjectionV1, PROJECTION_VERSION, ParseError, ProjectionTarget, known_element_type, normalize_document,
};

pub fn project_canvas(doc_bin: Vec<u8>, doc_id: String, revision: String) -> Result<CanvasProjectionV1, ParseError> {
  let normalized = normalize_document(doc_bin, ProjectionTarget::Canvas)?;
  let mut counts = BTreeMap::new();
  for element in &normalized.elements {
    *counts
      .entry(known_element_type(&element.element_type).to_string())
      .or_insert(0) += 1;
  }
  let bounds = aggregate_bounds(
    normalized
      .blocks
      .iter()
      .filter_map(|block| block.bounds.as_ref())
      .chain(normalized.elements.iter().filter_map(|element| element.bounds.as_ref())),
  );
  Ok(CanvasProjectionV1 {
    version: PROJECTION_VERSION,
    doc_id,
    revision,
    title: normalized.title,
    surface_block_id: normalized.surface_block_id,
    bounds,
    counts,
    blocks: normalized.blocks,
    elements: normalized.elements,
    warnings: normalized.warnings,
  })
}

fn aggregate_bounds<'a>(bounds: impl Iterator<Item = &'a Bounds>) -> Option<Bounds> {
  bounds.fold(None, |aggregate, item| match aggregate {
    None => Some(item.clone()),
    Some(current) => {
      let x = current.x.min(item.x);
      let y = current.y.min(item.y);
      let right = (current.x + current.width).max(item.x + item.width);
      let bottom = (current.y + current.height).max(item.y + item.height);
      Some(Bounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
      })
    }
  })
}
