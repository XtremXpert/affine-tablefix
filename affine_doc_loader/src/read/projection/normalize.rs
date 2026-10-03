use std::collections::HashMap;

use serde_json::Value as JsonValue;

use super::{
  ArrayRead, BOOKMARK_FLAVOURS, Bounds, CanvasBlock, CanvasElement, ImageSpec, MapRead, NOTE_FLAVOUR, ParseError,
  ProjectionWarning, TextRead, ValueRead, Visibility, build_block_index, collect_child_ids,
  collect_database_cell_references, compose_additional, determine_display_mode, embed_ref_payload,
  extract_inline_references, gather_database_texts, get_block_id, get_flavour, get_string, load_read_doc,
  nearest_by_flavour, params_value_to_json, table_cell_texts, value_to_string,
};

#[derive(Clone, Copy)]
pub(super) enum ProjectionTarget {
  Canvas,
  Search,
}

pub(super) struct NormalizedDocument {
  pub(super) title: String,
  pub(super) page_root_id: String,
  pub(super) surface_block_id: Option<String>,
  pub(super) blocks: Vec<CanvasBlock>,
  pub(super) search_blocks: Vec<NormalizedSearchBlock>,
  pub(super) elements: Vec<CanvasElement>,
  pub(super) warnings: Vec<ProjectionWarning>,
}

pub(super) struct NormalizedSearchBlock {
  pub(super) id: String,
  pub(super) block_type: String,
  pub(super) visibility: Visibility,
  pub(super) has_children: bool,
  pub(super) text: String,
  pub(super) blob_id: Option<String>,
  pub(super) ref_doc_ids: Vec<String>,
  pub(super) refs: Vec<String>,
  pub(super) parent_flavour: Option<String>,
  pub(super) parent_block_id: Option<String>,
  pub(super) additional: Option<String>,
}

pub(super) fn normalize_document(doc_bin: Vec<u8>, target: ProjectionTarget) -> Result<NormalizedDocument, ParseError> {
  let doc = load_read_doc(doc_bin)?;
  let blocks_map = doc
    .map("blocks")
    .ok_or_else(|| ParseError::ParserError("blocks map not found".into()))?;
  let index = build_block_index(&blocks_map);
  let mut warnings = Vec::new();
  let mut title = String::new();
  let mut page_root_id = String::new();
  let mut surface_block_id = None;
  let mut blocks = Vec::new();
  let mut search_blocks = Vec::new();
  let mut elements = Vec::new();
  let frame_membership = frame_membership(&index.block_pool);

  let mut block_ids = index.block_pool.keys().cloned().collect::<Vec<_>>();
  block_ids.sort();
  for block_id in block_ids {
    let block = &index.block_pool[&block_id];
    let Some(flavour) = get_flavour(block) else { continue };
    if flavour == "affine:page" {
      title = get_string(block, "prop:title").unwrap_or_default();
      page_root_id = block_id;
      continue;
    }
    if flavour == "affine:surface" {
      surface_block_id = Some(block_id.clone());
      elements.extend(normalize_surface_elements(block, &frame_membership, &mut warnings));
      continue;
    }
    let visibility = visibility_for(&block_id, &index.parent_lookup, &index.block_pool);
    let bounds = parse_bounds(block.get_value("prop:xywh").as_ref(), &block_id, &mut warnings);
    match target {
      ProjectionTarget::Canvas => {
        if let Some(block) = normalize_canvas_block(&block_id, &flavour, block, visibility, bounds, &index.block_pool) {
          blocks.push(block);
        }
      }
      ProjectionTarget::Search => {
        let parent_block_id = index.parent_lookup.get(&block_id).cloned();
        if let Some(block) = normalize_search_block(
          &block_id,
          &flavour,
          block,
          visibility,
          parent_block_id,
          &index.parent_lookup,
          &index.block_pool,
        ) {
          search_blocks.push(block);
        }
      }
    }
  }
  blocks.sort_by(|left, right| left.id.cmp(&right.id));
  search_blocks.sort_by(|left, right| left.id.cmp(&right.id));
  elements.sort_by(|left, right| left.id.cmp(&right.id));
  warnings.sort_by(|left, right| (&left.locator, &left.code).cmp(&(&right.locator, &right.code)));
  Ok(NormalizedDocument {
    title,
    page_root_id,
    surface_block_id,
    blocks,
    search_blocks,
    elements,
    warnings,
  })
}

fn normalize_canvas_block<M: MapRead>(
  block_id: &str,
  flavour: &str,
  block: &M,
  visibility: Visibility,
  bounds: Option<Bounds>,
  blocks: &HashMap<String, M>,
) -> Option<CanvasBlock> {
  let text = block_text(block).or_else(|| descendant_text(block_id, blocks));
  if !matches!(flavour, "affine:note" | "affine:frame" | "affine:edgeless-text") && text.is_none() {
    return None;
  }
  let block_type = match flavour {
    "affine:note" => "note",
    "affine:frame" => "frame",
    "affine:edgeless-text" => "edgeless-text",
    _ => flavour.strip_prefix("affine:").unwrap_or(flavour),
  };
  let child_ids = if flavour == "affine:frame" {
    map_keys(block, "prop:childElementIds")
  } else if matches!(flavour, "affine:note" | "affine:edgeless-text") {
    collect_child_ids(block)
  } else {
    Vec::new()
  };
  Some(CanvasBlock {
    id: block_id.to_string(),
    block_type: block_type.to_string(),
    visibility,
    bounds,
    text,
    title: get_string(block, "prop:title").filter(|value| !value.trim().is_empty()),
    child_ids,
  })
}

fn normalize_search_block<M: MapRead>(
  block_id: &str,
  flavour: &str,
  block: &M,
  visibility: Visibility,
  parent_block_id: Option<String>,
  parents: &HashMap<String, String>,
  blocks: &HashMap<String, M>,
) -> Option<NormalizedSearchBlock> {
  let note = nearest_by_flavour(block_id, NOTE_FLAVOUR, parents, blocks);
  let note_id = note.as_ref().and_then(get_block_id);
  let parent_flavour = parent_block_id
    .as_ref()
    .and_then(|parent_id| blocks.get(parent_id))
    .and_then(get_flavour);
  let database_name = if flavour == "affine:database" {
    get_string(block, "prop:title")
  } else if parent_flavour.as_deref() == Some("affine:database") {
    parent_block_id
      .as_ref()
      .and_then(|id| blocks.get(id))
      .and_then(|parent| get_string(parent, "prop:title"))
  } else {
    None
  };
  let mut ref_doc_ids = Vec::new();
  let mut refs = Vec::new();
  let mut blob_id = None;
  let text = match flavour {
    "affine:paragraph" | "affine:list" | "affine:code" => {
      let text = block.get_value("prop:text").and_then(|value| value.as_text_view());
      if let Some(text) = text {
        for reference in extract_inline_references(&text.delta()) {
          ref_doc_ids.push(reference.doc_id);
          refs.push(reference.payload);
        }
        text.text()
      } else {
        String::new()
      }
    }
    "affine:embed-linked-doc" | "affine:embed-synced-doc" => {
      if let Some(page_id) = get_string(block, "prop:pageId") {
        refs.extend(embed_ref_payload(block, &page_id));
        ref_doc_ids.push(page_id);
      }
      String::new()
    }
    "affine:attachment" => {
      blob_id = get_string(block, "prop:sourceId");
      get_string(block, "prop:name").unwrap_or_default()
    }
    "affine:image" => {
      let image = ImageSpec::from_block_map(block);
      blob_id = (!image.source_id.is_empty()).then_some(image.source_id);
      image.caption.unwrap_or_default()
    }
    "affine:database" => {
      for reference in collect_database_cell_references(block) {
        ref_doc_ids.push(reference.doc_id);
        refs.push(reference.payload);
      }
      gather_database_texts(block).0.join("\n")
    }
    "affine:table" => table_cell_texts(block).join("\n"),
    "affine:latex" => get_string(block, "prop:latex").unwrap_or_default(),
    "affine:note" | "affine:frame" | "affine:edgeless-text" => block_text(block)
      .or_else(|| get_string(block, "prop:title"))
      .or_else(|| descendant_text(block_id, blocks))
      .unwrap_or_default(),
    value if BOOKMARK_FLAVOURS.contains(&value) => String::new(),
    _ => block_text(block).unwrap_or_default(),
  };
  let indexable =
    !text.trim().is_empty() || blob_id.is_some() || !ref_doc_ids.is_empty() || BOOKMARK_FLAVOURS.contains(&flavour);
  indexable.then(|| NormalizedSearchBlock {
    id: block_id.to_string(),
    block_type: flavour.strip_prefix("affine:").unwrap_or(flavour).to_string(),
    visibility,
    has_children: !collect_child_ids(block).is_empty(),
    text,
    blob_id,
    ref_doc_ids,
    refs,
    parent_flavour,
    parent_block_id,
    additional: compose_additional(
      &determine_display_mode(note.as_ref()),
      note_id.as_ref(),
      database_name.as_ref(),
    ),
  })
}

fn normalize_surface_elements<M: MapRead>(
  surface: &M,
  frame_membership: &HashMap<String, String>,
  warnings: &mut Vec<ProjectionWarning>,
) -> Vec<CanvasElement> {
  let Some(elements) = surface.get_value("prop:elements").and_then(|value| value.as_map_view()) else {
    return Vec::new();
  };
  if get_string(&elements, "type").as_deref() != Some("$blocksuite:internal:native$") {
    return Vec::new();
  }
  let Some(values) = elements.get_value("value").and_then(|value| value.as_map_view()) else {
    return Vec::new();
  };
  let mut output = Vec::new();
  let mut mindmap_nodes = HashMap::new();
  for (entry_id, value) in values.entries() {
    let Some(element) = value.as_map_view() else { continue };
    let id = get_string(&element, "id").unwrap_or(entry_id);
    let element_type = get_string(&element, "type")
      .unwrap_or_else(|| "unknown".into())
      .to_lowercase();
    let known_type = known_element_type(&element_type).to_string();
    if known_type == "unknown" {
      warnings.push(ProjectionWarning {
        code: format!("UNKNOWN_ELEMENT_TYPE:{element_type}"),
        locator: id.clone(),
      });
    }
    let bounds = parse_bounds(element.get_value("xywh").as_ref(), &id, warnings);
    let child_ids = map_keys(&element, "children");
    if element_type == "mindmap" {
      let details = child_ids
        .iter()
        .filter_map(|child_id| {
          let detail = element
            .get_value("children")?
            .as_map_view()?
            .get_value(child_id)?
            .as_map_view()?;
          Some((
            child_id.clone(),
            get_string(&detail, "parent"),
            get_string(&detail, "index"),
          ))
        })
        .collect::<Vec<_>>();
      for (node_id, parent_id, index) in details {
        mindmap_nodes.insert(node_id, (parent_id, index));
      }
    }
    output.push(CanvasElement {
      id: id.clone(),
      element_type,
      bounds,
      text: get_string(&element, "text").or_else(|| get_string(&element, "label")),
      title: get_string(&element, "title"),
      frame_id: frame_membership.get(&id).cloned(),
      child_ids,
      source_id: connection_id(&element, "source"),
      target_id: connection_id(&element, "target"),
      parent_id: None,
      index: None,
      point_count: (known_type == "brush").then(|| brush_point_count(&element)),
      color: (known_type == "brush").then(|| get_string(&element, "color")).flatten(),
      line_width: (known_type == "brush").then(|| number(&element, "lineWidth")).flatten(),
    });
  }
  for element in &mut output {
    if let Some((parent_id, index)) = mindmap_nodes.get(&element.id) {
      element.parent_id.clone_from(parent_id);
      element.index.clone_from(index);
    }
  }
  output
}

fn frame_membership<M: MapRead>(blocks: &HashMap<String, M>) -> HashMap<String, String> {
  let mut membership = HashMap::new();
  let mut frames = blocks
    .iter()
    .filter(|(_, block)| get_flavour(*block).as_deref() == Some("affine:frame"))
    .collect::<Vec<_>>();
  frames.sort_by(|left, right| left.0.cmp(right.0));
  for (frame_id, frame) in frames {
    for child_id in map_keys(frame, "prop:childElementIds") {
      membership.entry(child_id).or_insert_with(|| frame_id.clone());
    }
  }
  membership
}

fn visibility_for<M: MapRead>(
  block_id: &str,
  parents: &HashMap<String, String>,
  blocks: &HashMap<String, M>,
) -> Visibility {
  let mut cursor = Some(block_id);
  while let Some(id) = cursor {
    if let Some(block) = blocks.get(id)
      && get_flavour(block).as_deref() == Some("affine:note")
    {
      return match get_string(block, "prop:displayMode").as_deref() {
        Some("both") => Visibility::Both,
        Some("page") => Visibility::Page,
        _ => Visibility::Edgeless,
      };
    }
    cursor = parents.get(id).map(String::as_str);
  }
  Visibility::Page
}

fn block_text<M: MapRead>(block: &M) -> Option<String> {
  ["prop:text", "prop:caption", "prop:latex", "prop:name"]
    .into_iter()
    .find_map(|key| get_string(block, key))
    .filter(|text| !text.trim().is_empty())
}

fn descendant_text<M: MapRead>(block_id: &str, blocks: &HashMap<String, M>) -> Option<String> {
  let block = blocks.get(block_id)?;
  let texts = collect_child_ids(block)
    .into_iter()
    .filter_map(|child_id| blocks.get(&child_id).and_then(block_text))
    .collect::<Vec<_>>();
  (!texts.is_empty()).then(|| texts.join("\n"))
}

fn parse_bounds<V: ValueRead>(
  value: Option<&V>,
  locator: &str,
  warnings: &mut Vec<ProjectionWarning>,
) -> Option<Bounds> {
  let value = value?;
  let json =
    params_value_to_json(value).or_else(|| value_to_string(value).and_then(|text| serde_json::from_str(&text).ok()));
  let values = json
    .as_ref()
    .and_then(JsonValue::as_array)
    .and_then(|values| (values.len() == 4).then(|| values.iter().filter_map(JsonValue::as_f64).collect::<Vec<_>>()));
  let Some(values) = values.filter(|values| {
    values.len() == 4 && values.iter().all(|value| value.is_finite()) && values[2] >= 0.0 && values[3] >= 0.0
  }) else {
    warnings.push(ProjectionWarning {
      code: "INVALID_BOUNDS".into(),
      locator: locator.into(),
    });
    return None;
  };
  Some(Bounds {
    x: values[0],
    y: values[1],
    width: values[2],
    height: values[3],
  })
}

fn map_keys<M: MapRead>(map: &M, key: &str) -> Vec<String> {
  let mut keys: Vec<String> = map
    .get_value(key)
    .and_then(|value| value.as_map_view())
    .map(|value| value.keys().map(str::to_string).collect())
    .unwrap_or_default();
  keys.sort();
  keys
}

fn connection_id<M: MapRead>(element: &M, key: &str) -> Option<String> {
  element.get_value(key).and_then(|value| {
    value
      .as_map_view()
      .and_then(|connection| get_string(&connection, "id"))
      .or_else(|| {
        params_value_to_json(&value).and_then(|json| json.get("id").and_then(JsonValue::as_str).map(str::to_string))
      })
  })
}

fn number<M: MapRead>(map: &M, key: &str) -> Option<f64> {
  map
    .get_value(key)
    .and_then(|value| params_value_to_json(&value))
    .and_then(|value| value.as_f64())
}

fn brush_point_count<M: MapRead>(map: &M) -> u32 {
  let Some(points) = map.get_value("points").and_then(|value| value.as_array_view()) else {
    return 0;
  };
  let points = points.items().collect::<Vec<_>>();
  if points.iter().any(|point| point.as_array_view().is_some()) {
    points.len() as u32
  } else {
    points.len().div_ceil(2) as u32
  }
}

pub(super) fn known_element_type(value: &str) -> &str {
  match value {
    "shape" | "text" | "connector" | "group" | "brush" | "mindmap" => value,
    _ => "unknown",
  }
}
