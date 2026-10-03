use std::collections::HashSet;

use super::{
  AnyRead, ArrayRead, DeltaToMdOptions, DocContext, InlineReferencePayload, MapRead, MarkdownTableOptions, TextRead,
  ValueRead, collect_child_ids, delta_value_to_inline_markdown, extract_inline_references_from_value, get_string,
  render_markdown_table, text_content, text_content_for_summary, text_to_inline_markdown, value_to_string,
};

pub(super) struct DatabaseTable {
  columns: Vec<DatabaseColumn>,
  rows: Vec<Vec<String>>,
}

struct DatabaseOption {
  id: Option<String>,
  value: Option<String>,
  color: Option<String>,
}

struct DatabaseColumn {
  id: String,
  name: Option<String>,
  col_type: String,
  options: Vec<DatabaseOption>,
}

pub(super) fn build_database_table<M: MapRead>(
  block: &M,
  context: &DocContext<M>,
  md_options: &DeltaToMdOptions,
) -> Option<DatabaseTable> {
  let columns = parse_database_columns(block)?;
  let cells_map = block.get_value("prop:cells").and_then(|value| value.as_map_view())?;
  let child_ids = collect_child_ids(block);

  let mut rows = Vec::new();
  for child_id in child_ids {
    let row_cells = cells_map.get_value(&child_id).and_then(|value| value.as_map_view());
    let mut row = Vec::new();

    for column in columns.iter() {
      let mut cell_text = String::new();
      if column.col_type == "title" {
        if let Some(child_block) = context.block_pool.get(&child_id) {
          if let Some(text_md) = text_to_inline_markdown(child_block, "prop:text", md_options) {
            cell_text = text_md;
          } else if let Some((text, _)) = text_content(child_block, "prop:text") {
            cell_text = text;
          } else if let Some((text, _)) = text_content_for_summary(child_block, "prop:text") {
            cell_text = text;
          }
        }
      } else if let Some(row_cells) = &row_cells
        && let Some(cell_val) = row_cells.get_value(&column.id).and_then(|value| value.as_map_view())
        && let Some(value) = cell_val.get_value("value")
      {
        if let Some(text_md) = delta_value_to_inline_markdown(&value, md_options) {
          cell_text = text_md;
        } else {
          cell_text = format_cell_value(&value, column);
        }
      }

      row.push(cell_text);
    }
    if row.iter().any(|cell| !cell.is_empty()) {
      rows.push(row);
    }
  }

  Some(DatabaseTable { columns, rows })
}

pub(super) fn database_table_markdown(table: DatabaseTable) -> Option<String> {
  let mut rows = Vec::with_capacity(table.rows.len() + 1);
  let header = table
    .columns
    .iter()
    .map(|column| column.name.as_deref().unwrap_or_default().to_string())
    .collect::<Vec<_>>();
  rows.push(header);
  rows.extend(table.rows);

  let options = MarkdownTableOptions::new(false, "<br />", true);
  render_markdown_table(&rows, options)
}

pub(super) fn database_summary_text<M: MapRead>(block: &M, context: &DocContext<M>) -> Option<String> {
  let md_options = DeltaToMdOptions::new(None);
  let table = build_database_table(block, context, &md_options)?;
  let mut summary = String::new();

  if let Some(title) = get_string(block, "prop:title")
    && !title.is_empty()
  {
    summary.push_str(&title);
    summary.push('|');
  }

  for column in table.columns.iter() {
    if let Some(name) = column.name.as_ref()
      && !name.is_empty()
    {
      summary.push_str(name);
      summary.push('|');
    }
    for option in column.options.iter() {
      if let Some(value) = option.value.as_ref()
        && !value.is_empty()
      {
        summary.push_str(value);
        summary.push('|');
      }
    }
  }

  for row in table.rows.iter() {
    for cell_text in row.iter() {
      if !cell_text.is_empty() {
        summary.push_str(cell_text);
        summary.push('|');
      }
    }
  }

  if summary.is_empty() { None } else { Some(summary) }
}

pub(super) fn gather_database_texts(block: &impl MapRead) -> (Vec<String>, Option<String>) {
  let mut texts = Vec::new();
  let database_title = get_string(block, "prop:title");
  if let Some(title) = &database_title {
    texts.push(title.clone());
  }

  if let Some(columns) = parse_database_columns(block) {
    for column in columns.iter() {
      if let Some(name) = column.name.as_ref() {
        texts.push(name.clone());
      }
      for option in column.options.iter() {
        if let Some(value) = option.value.as_ref() {
          texts.push(value.clone());
        }
      }
    }
  }

  (texts, database_title)
}

pub(super) fn collect_database_cell_references(block: &impl MapRead) -> Vec<InlineReferencePayload> {
  let cells_map = match block.get_value("prop:cells").and_then(|value| value.as_map_view()) {
    Some(map) => map,
    None => return Vec::new(),
  };

  let mut refs = Vec::new();
  let mut seen: HashSet<(String, String)> = HashSet::new();

  for row in cells_map.values() {
    let Some(row_map) = row.as_map_view() else {
      continue;
    };
    for cell in row_map.values() {
      let Some(cell_map) = cell.as_map_view() else {
        continue;
      };
      let Some(value) = cell_map.get_value("value") else {
        continue;
      };
      for reference in extract_inline_references_from_value(&value) {
        let key = (reference.doc_id.clone(), reference.payload.clone());
        if seen.insert(key) {
          refs.push(reference);
        }
      }
    }
  }

  refs
}

fn parse_database_columns(block: &impl MapRead) -> Option<Vec<DatabaseColumn>> {
  let columns = block
    .get_value("prop:columns")
    .and_then(|value| value.as_array_view())?;
  let mut parsed = Vec::new();
  for column_value in columns.items() {
    if let Some(column) = column_value.as_map_view() {
      let id = column
        .get_value("id")
        .and_then(|value| value_to_string(&value))
        .unwrap_or_default();
      let name = column.get_value("name").and_then(|value| value_to_string(&value));
      let col_type = column
        .get_value("type")
        .and_then(|value| value_to_string(&value))
        .unwrap_or_default();
      let options = parse_database_options(&column);
      parsed.push(DatabaseColumn {
        id,
        name,
        col_type,
        options,
      });
    }
  }
  Some(parsed)
}

fn parse_database_options(column: &impl MapRead) -> Vec<DatabaseOption> {
  let Some(data) = column.get_value("data").and_then(|value| value.as_map_view()) else {
    return Vec::new();
  };
  let Some(options) = data.get_value("options").and_then(|value| value.as_array_view()) else {
    return Vec::new();
  };

  let mut parsed = Vec::new();
  for option_value in options.items() {
    if let Some(option) = option_value.as_map_view() {
      parsed.push(DatabaseOption {
        id: option.get_value("id").and_then(|value| value_to_string(&value)),
        value: option.get_value("value").and_then(|value| value_to_string(&value)),
        color: option.get_value("color").and_then(|value| value_to_string(&value)),
      });
    }
  }
  parsed
}

fn format_option_tag(option: &DatabaseOption) -> String {
  let id = option.id.as_deref().unwrap_or_default();
  let value = option.value.as_deref().unwrap_or_default();
  let color = option.color.as_deref().unwrap_or_default();

  format!("<span data-affine-option data-value=\"{id}\" data-option-color=\"{color}\">{value}</span>")
}

fn format_cell_value(value: &impl ValueRead, column: &DatabaseColumn) -> String {
  match column.col_type.as_str() {
    "select" => {
      let id = value
        .as_any_read()
        .and_then(|any| any.as_str().map(str::to_string))
        .or_else(|| value.as_text_view().map(|text| text.text()));
      if let Some(id) = id {
        for option in column.options.iter() {
          if option.id.as_deref() == Some(id.as_str()) {
            return format_option_tag(option);
          }
        }
      }
      String::new()
    }
    "multi-select" => {
      let ids = value
        .as_any_read()
        .and_then(|any| any.string_array())
        .or_else(|| {
          value
            .as_array_view()
            .map(|array| array.items().filter_map(|id| value_to_string(&id)).collect())
        })
        .unwrap_or_default();

      if ids.is_empty() {
        return String::new();
      }

      let mut selected = Vec::new();
      for id in ids.iter() {
        for option in column.options.iter() {
          if option.id.as_deref() == Some(id.as_str()) {
            selected.push(format_option_tag(option));
          }
        }
      }
      selected.join("")
    }
    _ => value_to_string(value).unwrap_or_default(),
  }
}
