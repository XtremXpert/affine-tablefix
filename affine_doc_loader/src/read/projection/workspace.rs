use y_octo::Any;

use super::{
  ArrayRead, MapRead, ParseError, ValueRead, WorkspaceRootProjection, any_as_string, any_truthy, get_string, load_doc,
};

pub fn project_workspace_root(doc_bin: Vec<u8>, include_trash: bool) -> Result<WorkspaceRootProjection, ParseError> {
  let doc = load_doc(&doc_bin, None)?;
  let complete = !doc.has_pending_updates();
  let doc_ids = doc
    .get_map("meta")
    .map(|meta| collect_doc_ids(meta, include_trash))
    .unwrap_or_default();
  Ok(WorkspaceRootProjection { doc_ids, complete })
}

pub fn get_doc_ids_from_binary(doc_bin: Vec<u8>, include_trash: bool) -> Result<Vec<String>, ParseError> {
  project_workspace_root(doc_bin, include_trash).map(|projection| projection.doc_ids)
}

fn collect_doc_ids<M: MapRead>(meta: M, include_trash: bool) -> Vec<String> {
  let mut doc_ids = Vec::new();
  let pages_value = meta.get_value("pages");
  if let Some(pages) = pages_value.as_ref().and_then(ValueRead::as_array_view) {
    for page_val in pages.items() {
      if let Some(page) = page_val.as_map_view()
        && let Some(id) = get_string(&page, "id")
      {
        let trash = page
          .get_value("trash")
          .and_then(|value| value.as_any_read().map(|value| any_truthy(&value)))
          .unwrap_or(false);
        if include_trash || !trash {
          doc_ids.push(id);
        }
      }
    }
    return doc_ids;
  }
  if let Some(Any::Array(entries)) = pages_value.and_then(|value| value.as_any_owned()) {
    for entry in entries {
      let Any::Object(map) = entry else {
        continue;
      };
      if let Some(id) = map.get("id").and_then(any_as_string).map(str::to_string) {
        let trash = map.get("trash").map(any_truthy).unwrap_or(false);
        if include_trash || !trash {
          doc_ids.push(id);
        }
      }
    }
  }
  doc_ids
}
