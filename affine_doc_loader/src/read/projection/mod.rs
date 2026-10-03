mod canvas;
mod normalize;
mod search;
mod types;
mod workspace;

pub use canvas::project_canvas;
use normalize::{ProjectionTarget, known_element_type, normalize_document};
pub use search::project_document_search;
pub use types::{
  Bounds, CanvasBlock, CanvasElement, CanvasProjectionV1, DocumentSearchProjectionV1, DocumentSearchUnit,
  ProjectionWarning, SearchUnitSource, Visibility, WorkspaceRootProjection,
};
pub use workspace::{get_doc_ids_from_binary, project_workspace_root};

use super::{
  ArrayRead, BOOKMARK_FLAVOURS, ImageSpec, MapRead, NOTE_FLAVOUR, ParseError, TextRead, ValueRead, any_as_string,
  any_truthy, build_block_index, collect_child_ids, collect_database_cell_references, compose_additional,
  determine_display_mode, embed_ref_payload, extract_inline_references, gather_database_texts, get_block_id,
  get_flavour, get_string, load_doc, load_read_doc, nearest_by_flavour, params_value_to_json, table_cell_texts,
  value_to_string,
};

const PROJECTION_VERSION: u8 = 1;
