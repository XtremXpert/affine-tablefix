use y_octo::{Any, DocOptions, Map};

use super::{
  super::{doc_loader::is_empty_doc, value::value_to_string},
  ParseError,
};

pub fn update_doc_properties(
  existing_binary: &[u8],
  properties_doc_id: &str,
  target_doc_id: &str,
  created_by: Option<&str>,
  updated_by: Option<&str>,
) -> Result<Vec<u8>, ParseError> {
  let doc = if is_empty_doc(existing_binary) {
    DocOptions::new().with_guid(properties_doc_id.to_string()).build()
  } else {
    super::load_doc(existing_binary, Some(properties_doc_id))?
  };

  let state_before = doc.get_state_vector();
  let mut record = doc.get_or_create_map(target_doc_id)?;
  let mut changed = false;

  if record.get("id").is_none() {
    record.insert("id".to_string(), Any::String(target_doc_id.to_string()))?;
    changed = true;
  }

  if let Some(created_by) = created_by
    && get_record_string(&record, "createdBy").as_deref() != Some(created_by)
  {
    record.insert("createdBy".to_string(), Any::String(created_by.to_string()))?;
    changed = true;
  }

  if let Some(updated_by) = updated_by
    && get_record_string(&record, "updatedBy").as_deref() != Some(updated_by)
  {
    record.insert("updatedBy".to_string(), Any::String(updated_by.to_string()))?;
    changed = true;
  }

  if !changed {
    return Ok(Vec::new());
  }

  Ok(doc.encode_state_as_update_v1(&state_before)?)
}

fn get_record_string(record: &Map, key: &str) -> Option<String> {
  record.get(key).and_then(|value| value_to_string(&value))
}

#[cfg(test)]
#[path = "../tests/write/doc_properties.rs"]
mod tests;
