use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
  Page,
  Edgeless,
  Both,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionWarning {
  pub code: String,
  pub locator: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct WorkspaceRootProjection {
  pub doc_ids: Vec<String>,
  pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasBlock {
  pub id: String,
  #[serde(rename = "type")]
  pub block_type: String,
  pub visibility: Visibility,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub bounds: Option<Bounds>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub text: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(skip_serializing_if = "Vec::is_empty", default)]
  pub child_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasElement {
  pub id: String,
  #[serde(rename = "type")]
  pub element_type: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub bounds: Option<Bounds>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub text: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub frame_id: Option<String>,
  #[serde(skip_serializing_if = "Vec::is_empty", default)]
  pub child_ids: Vec<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub source_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub parent_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub index: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub point_count: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub color: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub line_width: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasProjectionV1 {
  pub version: u8,
  pub doc_id: String,
  pub revision: String,
  pub title: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub surface_block_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub bounds: Option<Bounds>,
  pub counts: BTreeMap<String, u32>,
  pub blocks: Vec<CanvasBlock>,
  pub elements: Vec<CanvasElement>,
  pub warnings: Vec<ProjectionWarning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchUnitSource {
  PageBlock,
  CanvasBlock,
  SurfaceElement,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSearchUnit {
  pub unit_id: String,
  pub source: SearchUnitSource,
  pub visibility: Visibility,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub block_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub element_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub frame_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub blob_id: Option<String>,
  #[serde(skip_serializing_if = "Vec::is_empty", default)]
  pub ref_doc_ids: Vec<String>,
  #[serde(skip_serializing_if = "Vec::is_empty", default)]
  pub refs: Vec<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub parent_flavour: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub parent_block_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub additional: Option<String>,
  #[serde(rename = "type")]
  pub unit_type: String,
  pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSearchProjectionV1 {
  pub version: u8,
  pub doc_id: String,
  pub revision: String,
  pub source_hash: String,
  pub title: String,
  pub units: Vec<DocumentSearchUnit>,
  pub warnings: Vec<ProjectionWarning>,
}
