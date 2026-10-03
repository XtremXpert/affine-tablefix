use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;
use y_octo::{Doc, DocOptions};

const DELETED_FIELD: &str = "$$DELETED";

#[derive(Debug, Error)]
pub enum WorkspaceProjectionError {
  #[error("invalid ydoc update")]
  InvalidUpdate,
  #[error("invalid workspace fact: {0}")]
  InvalidFact(&'static str),
  #[error("unsupported collection filter: {filter_type}:{key}:{method}")]
  UnsupportedFilter {
    filter_type: String,
    key: String,
    method: String,
  },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagFact {
  pub id: String,
  pub name: String,
  pub document_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionFilter {
  #[serde(rename = "type")]
  pub filter_type: String,
  pub key: String,
  pub method: String,
  pub value: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionFact {
  pub id: String,
  pub name: String,
  #[serde(default)]
  pub filters: Vec<CollectionFilter>,
  #[serde(default)]
  pub allow_list: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFacts {
  pub id: String,
  #[serde(default)]
  pub title: String,
  pub created_at: Option<i64>,
  pub updated_at: Option<i64>,
  #[serde(default)]
  pub tags: Vec<String>,
  #[serde(default)]
  pub trash: bool,
  #[serde(default)]
  pub favorite: bool,
  #[serde(default)]
  pub shared: bool,
  pub primary_mode: Option<String>,
  pub journal: Option<String>,
  pub integration_type: Option<String>,
  #[serde(default)]
  pub properties: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFacts {
  pub complete: bool,
  pub documents: Vec<DocumentFacts>,
  pub tags: Vec<TagFact>,
  pub collections: Vec<CollectionFact>,
}

pub fn project_workspace_root_facts(binary: &[u8]) -> Result<WorkspaceFacts, WorkspaceProjectionError> {
  let root =
    crate::project_workspace_root(binary.to_vec(), true).map_err(|_| WorkspaceProjectionError::InvalidUpdate)?;
  let doc = load_doc(binary)?;
  let meta = doc
    .get_map("meta")
    .map_err(|_| WorkspaceProjectionError::InvalidFact("meta"))?;
  let meta_json = serde_json::to_value(meta).map_err(|_| WorkspaceProjectionError::InvalidFact("meta"))?;
  let pages = meta_json.get("pages").and_then(JsonValue::as_array);
  let documents = root
    .doc_ids
    .iter()
    .map(|id| {
      let page = pages.and_then(|pages| {
        pages
          .iter()
          .find(|page| page.get("id").and_then(JsonValue::as_str) == Some(id))
      });
      DocumentFacts {
        id: id.clone(),
        title: page
          .and_then(|value| value.get("title"))
          .and_then(JsonValue::as_str)
          .unwrap_or_default()
          .to_string(),
        created_at: page.and_then(|value| value.get("createDate")).and_then(json_i64),
        updated_at: page.and_then(|value| value.get("updatedDate")).and_then(json_i64),
        tags: page
          .and_then(|value| value.get("tags"))
          .and_then(JsonValue::as_array)
          .map(|values| string_values(values))
          .unwrap_or_default(),
        trash: page
          .and_then(|value| value.get("trash"))
          .and_then(JsonValue::as_bool)
          .unwrap_or(false),
        ..Default::default()
      }
    })
    .collect::<Vec<_>>();
  let tag_options = meta_json
    .pointer("/properties/tags/options")
    .and_then(JsonValue::as_array)
    .cloned()
    .unwrap_or_default();
  let tags = tag_options
    .iter()
    .filter_map(|tag| {
      let id = tag.get("id")?.as_str()?.to_string();
      Some(TagFact {
        name: tag
          .get("value")
          .and_then(JsonValue::as_str)
          .unwrap_or_default()
          .to_string(),
        document_ids: documents
          .iter()
          .filter(|doc| doc.tags.contains(&id))
          .map(|doc| doc.id.clone())
          .collect(),
        id,
      })
    })
    .collect();
  let settings = doc
    .get_map("setting")
    .ok()
    .and_then(|map| serde_json::to_value(map).ok())
    .unwrap_or(JsonValue::Null);
  let collections = settings
    .get("collections")
    .and_then(JsonValue::as_array)
    .into_iter()
    .flatten()
    .filter_map(parse_collection)
    .collect();
  Ok(WorkspaceFacts {
    complete: root.complete,
    documents,
    tags,
    collections,
  })
}

pub fn project_orm_records(binary: &[u8]) -> Result<Vec<JsonValue>, WorkspaceProjectionError> {
  let doc = load_doc(binary)?;
  Ok(
    doc
      .keys()
      .into_iter()
      .filter_map(|key| doc.get_map(&key).ok())
      .filter_map(|record| serde_json::to_value(record).ok())
      .filter(|record| !record.get(DELETED_FIELD).and_then(JsonValue::as_bool).unwrap_or(false))
      .filter(|record| record.as_object().is_some_and(|object| !object.is_empty()))
      .collect::<Vec<_>>(),
  )
}

pub fn apply_workspace_db(documents: &mut [DocumentFacts], records: &[JsonValue]) {
  for record in records {
    let Some(id) = record.get("id").and_then(JsonValue::as_str) else {
      continue;
    };
    let Some(document) = documents.iter_mut().find(|document| document.id == id) else {
      continue;
    };
    document.primary_mode = record
      .get("primaryMode")
      .and_then(JsonValue::as_str)
      .map(str::to_string);
    document.journal = record.get("journal").and_then(JsonValue::as_str).map(str::to_string);
    document.integration_type = record
      .get("integrationType")
      .and_then(JsonValue::as_str)
      .map(str::to_string);
    for (key, value) in record.as_object().into_iter().flatten() {
      if key.starts_with("custom:") {
        document.properties.insert(key.clone(), value.clone());
      }
    }
  }
}

pub fn apply_favorites(documents: &mut [DocumentFacts], records: &[JsonValue]) {
  let favorites = records
    .iter()
    .filter_map(|record| record.get("key").and_then(JsonValue::as_str))
    .filter_map(|key| key.strip_prefix("doc:"))
    .collect::<BTreeSet<_>>();
  for document in documents {
    document.favorite = favorites.contains(document.id.as_str());
  }
}

pub fn evaluate_collection(
  collection: &CollectionFact,
  documents: &[DocumentFacts],
  now: DateTime<Utc>,
) -> Result<Vec<String>, WorkspaceProjectionError> {
  let mut result = collection.allow_list.iter().cloned().collect::<BTreeSet<_>>();
  if collection.filters.is_empty() {
    return Ok(result.into_iter().collect());
  }
  for document in documents {
    if collection.filters.iter().try_fold(true, |matched, filter| {
      if !matched {
        Ok(false)
      } else {
        matches_filter(document, filter, now)
      }
    })? {
      result.insert(document.id.clone());
    }
  }
  Ok(result.into_iter().collect())
}

fn matches_filter(
  document: &DocumentFacts,
  filter: &CollectionFilter,
  now: DateTime<Utc>,
) -> Result<bool, WorkspaceProjectionError> {
  let value = filter.value.as_deref();
  let matched = match (filter.filter_type.as_str(), filter.key.as_str(), filter.method.as_str()) {
    ("system", "favorite", "is") => document.favorite == parse_bool(value)?,
    ("system", "shared", "is") => document.shared == parse_bool(value)?,
    ("system", "trash", "is") => document.trash == parse_bool(value)?,
    ("system", "title", "match") => document
      .title
      .to_lowercase()
      .contains(&value.unwrap_or_default().to_lowercase()),
    ("system", "createdAt", method @ ("before" | "after" | "last")) => {
      date_matches(document.created_at, method, value, now)?
    }
    ("system", "updatedAt", method @ ("before" | "after" | "last")) => {
      date_matches(document.updated_at, method, value, now)?
    }
    ("system", "tags", "is-empty") => document.tags.is_empty(),
    ("system", "tags", "is-not-empty") => !document.tags.is_empty(),
    ("system", "tags", method @ ("include-all" | "include-any-of" | "not-include-all" | "not-include-any-of")) => {
      let expected = value
        .unwrap_or_default()
        .split(',')
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
      let actual = document.tags.iter().map(String::as_str).collect::<BTreeSet<_>>();
      match method {
        "include-all" => expected.is_subset(&actual),
        "include-any-of" => !expected.is_disjoint(&actual),
        "not-include-all" => !expected.is_subset(&actual),
        _ => expected.is_disjoint(&actual),
      }
    }
    ("system", "journal", method @ ("is" | "is-not")) => {
      compare_bool(method, document.journal.is_some(), parse_bool(value)?)
    }
    ("system", "empty-journal", "is") => {
      (document.journal.is_some() && document.updated_at.is_none()) == parse_bool(value)?
    }
    ("system", "docPrimaryMode", method @ ("is" | "is-not")) => compare_string(
      method,
      document.primary_mode.as_deref().unwrap_or("page"),
      value.unwrap_or_default(),
    ),
    ("system", "integrationType", method @ ("is" | "is-not")) => compare_string(
      method,
      document.integration_type.as_deref().unwrap_or_default(),
      value.unwrap_or_default(),
    ),
    ("property", key, method) => property_matches(document.properties.get(key), method, value)?,
    _ => return Err(unsupported(filter)),
  };
  Ok(matched)
}

fn property_matches(
  value: Option<&JsonValue>,
  method: &str,
  expected: Option<&str>,
) -> Result<bool, WorkspaceProjectionError> {
  let expected = expected.unwrap_or_default();
  let text = value.and_then(JsonValue::as_str).unwrap_or_default();
  Ok(match method {
    "is" | "=" => text == expected,
    "is-not" | "≠" => text != expected,
    "contains" => text.contains(expected),
    "is-empty" => value.is_none() || value == Some(&JsonValue::Null) || text.is_empty(),
    "is-not-empty" => value.is_some() && value != Some(&JsonValue::Null) && !text.is_empty(),
    _ => {
      return Err(WorkspaceProjectionError::UnsupportedFilter {
        filter_type: "property".to_string(),
        key: String::new(),
        method: method.to_string(),
      });
    }
  })
}

fn date_matches(
  timestamp: Option<i64>,
  method: &str,
  value: Option<&str>,
  now: DateTime<Utc>,
) -> Result<bool, WorkspaceProjectionError> {
  let Some(timestamp) = timestamp.and_then(DateTime::from_timestamp_millis) else {
    return Ok(false);
  };
  let date = timestamp.date_naive();
  if method == "last" {
    let days = value
      .and_then(|value| value.parse::<i64>().ok())
      .ok_or(WorkspaceProjectionError::InvalidFact("date"))?;
    return Ok(date >= (now - Duration::days(days)).date_naive());
  }
  let expected = value
    .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
    .ok_or(WorkspaceProjectionError::InvalidFact("date"))?;
  Ok(if method == "before" {
    date < expected
  } else {
    date > expected
  })
}

fn parse_collection(value: &JsonValue) -> Option<CollectionFact> {
  let id = value.get("id")?.as_str()?.to_string();
  let filters = value
    .pointer("/rules/filters")
    .and_then(JsonValue::as_array)
    .and_then(|filters| serde_json::from_value(JsonValue::Array(filters.clone())).ok())
    .unwrap_or_default();
  Some(CollectionFact {
    id,
    name: value
      .get("name")
      .and_then(JsonValue::as_str)
      .unwrap_or_default()
      .to_string(),
    filters,
    allow_list: value
      .get("allowList")
      .and_then(JsonValue::as_array)
      .map(|values| string_values(values))
      .unwrap_or_default(),
  })
}

fn load_doc(binary: &[u8]) -> Result<Doc, WorkspaceProjectionError> {
  let mut doc = DocOptions::new().build();
  doc
    .apply_update_from_binary_v1(binary)
    .map_err(|_| WorkspaceProjectionError::InvalidUpdate)?;
  Ok(doc)
}

fn json_i64(value: &JsonValue) -> Option<i64> {
  value.as_i64().or_else(|| value.as_f64().map(|value| value as i64))
}

fn string_values(values: &[JsonValue]) -> Vec<String> {
  values
    .iter()
    .filter_map(JsonValue::as_str)
    .map(str::to_string)
    .collect()
}

fn parse_bool(value: Option<&str>) -> Result<bool, WorkspaceProjectionError> {
  match value {
    Some("true") => Ok(true),
    Some("false") => Ok(false),
    _ => Err(WorkspaceProjectionError::InvalidFact("boolean filter value")),
  }
}

fn compare_bool(method: &str, actual: bool, expected: bool) -> bool {
  if method == "is" {
    actual == expected
  } else {
    actual != expected
  }
}

fn compare_string(method: &str, actual: &str, expected: &str) -> bool {
  if method == "is" {
    actual == expected
  } else {
    actual != expected
  }
}

fn unsupported(filter: &CollectionFilter) -> WorkspaceProjectionError {
  WorkspaceProjectionError::UnsupportedFilter {
    filter_type: filter.filter_type.clone(),
    key: filter.key.clone(),
    method: filter.method.clone(),
  }
}

#[cfg(test)]
#[path = "tests/workspace.rs"]
mod tests;
