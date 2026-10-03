//! Regression test for tables written from markdown (toeverything/AFFiNE#15466):
//! cells must be Y.Text and row/column orders must be fractional-indexing keys,
//! otherwise the editor renders empty cells and cannot insert rows or columns.

use affine_doc_loader::{build_full_doc, update_doc};
use y_octo::{Any, Doc, DocOptions, Map, Value};

const DOC_ID: &str = "table-doc";

/// Returns the doc with the table block: the map is only readable while its doc is alive.
fn table_block(binary: &[u8]) -> (Doc, Map) {
  let mut doc = DocOptions::new().with_guid(DOC_ID.to_string()).build();
  doc.apply_update_from_binary_v1(binary).expect("valid update");
  let blocks = doc.get_map("blocks").expect("blocks map");
  let block = blocks
    .values()
    .filter_map(|value| value.to_map())
    .find(|block| matches!(block.get("sys:flavour"), Some(Value::Any(Any::String(f))) if f == "affine:table"))
    .expect("table block");
  (doc, block)
}

fn orders(block: &Map, prefix: &str) -> Vec<String> {
  let mut orders: Vec<(String, String)> = block
    .keys()
    .filter(|key| key.starts_with(prefix) && key.ends_with(".order"))
    .map(|key| {
      let order = match block.get(key) {
        Some(Value::Any(Any::String(order))) => order,
        other => panic!("order {key} is not a string: {other:?}"),
      };
      (key.to_string(), order)
    })
    .collect();
  orders.sort_by(|a, b| a.1.cmp(&b.1));
  orders.into_iter().map(|(_, order)| order).collect()
}

fn cell_texts(block: &Map) -> Vec<String> {
  let mut texts: Vec<String> = block
    .keys()
    .filter(|key| key.starts_with("prop:cells.") && key.ends_with(".text"))
    .map(|key| {
      block
        .get(key)
        .and_then(|value| value.to_text())
        .unwrap_or_else(|| panic!("cell {key} is not a Y.Text"))
        .to_string()
    })
    .collect();
  texts.sort();
  texts
}

fn assert_valid_order_keys(keys: &[String], expected: usize) {
  assert_eq!(keys.len(), expected);
  for key in keys {
    let head = key.as_bytes()[0];
    assert!(head.is_ascii_lowercase(), "order key {key} must start with a-z");
    assert_eq!(key.len(), (head - b'a') as usize + 2, "order key {key} has a wrong length");
  }
}

#[test]
fn created_table_cells_are_text_with_fractional_orders() {
  let markdown = "| Nom | Valeur |\n|---|---|\n| a | 1 |\n| b | 2 |\n";
  let binary = build_full_doc("Table", markdown, DOC_ID).expect("doc");
  let (_doc, block) = table_block(&binary);

  assert_eq!(cell_texts(&block), ["1", "2", "Nom", "Valeur", "a", "b"]);
  let rows = orders(&block, "prop:rows.");
  assert_eq!(rows, ["a0", "a1", "a2"]);
  assert_valid_order_keys(&rows, 3);
  assert_eq!(orders(&block, "prop:columns."), ["a0", "a1"]);
}

#[test]
fn many_rows_keep_their_order() {
  let mut markdown = String::from("| n |\n|---|\n");
  for i in 0..99 {
    markdown.push_str(&format!("| {i} |\n"));
  }
  let binary = build_full_doc("Table", &markdown, DOC_ID).expect("doc");
  let (_doc, block) = table_block(&binary);
  let rows = orders(&block, "prop:rows.");
  assert_valid_order_keys(&rows, 100);
  assert_eq!(rows[61], "az");
  assert_eq!(rows[62], "b00");

  // Rows sorted by order key must come back in markdown order.
  let mut by_order: Vec<(String, String)> = block
    .keys()
    .filter(|key| key.starts_with("prop:rows.") && key.ends_with(".order"))
    .map(|key| {
      let row_id = key.trim_start_matches("prop:rows.").trim_end_matches(".order").to_string();
      let order = match block.get(key) {
        Some(Value::Any(Any::String(order))) => order,
        _ => unreachable!(),
      };
      (order, row_id)
    })
    .collect();
  by_order.sort();
  let column_key = block
    .keys()
    .find(|key| key.starts_with("prop:columns.") && key.ends_with(".columnId"))
    .unwrap()
    .to_string();
  let column_id = match block.get(&column_key) {
    Some(Value::Any(Any::String(id))) => id,
    _ => unreachable!(),
  };
  let values: Vec<String> = by_order
    .iter()
    .map(|(_, row_id)| {
      block
        .get(&format!("prop:cells.{row_id}:{column_id}.text"))
        .and_then(|value| value.to_text())
        .unwrap()
        .to_string()
    })
    .collect();
  let mut expected = vec!["n".to_string()];
  expected.extend((0..99).map(|i| i.to_string()));
  assert_eq!(values, expected);
}

#[test]
fn updated_table_cells_are_text() {
  let binary = build_full_doc("Table", "intro\n", DOC_ID).expect("doc");
  let delta = update_doc(&binary, "intro\n\n| k | v |\n|---|---|\n| x | y |\n", DOC_ID).expect("update");
  let mut doc = DocOptions::new().with_guid(DOC_ID.to_string()).build();
  doc.apply_update_from_binary_v1(&binary).unwrap();
  doc.apply_update_from_binary_v1(&delta).unwrap();
  let full = doc.encode_update_v1().unwrap();
  let (_doc, block) = table_block(&full);
  assert_eq!(cell_texts(&block), ["k", "v", "x", "y"]);
  assert_eq!(orders(&block, "prop:rows."), ["a0", "a1"]);
}


#[test]
fn table_round_trips_to_markdown() {
  let markdown = "| Nom | Valeur |\n|---|---|\n| a | 1 |\n";
  let binary = build_full_doc("Table", markdown, DOC_ID).expect("doc");
  let result = affine_doc_loader::parse_doc_to_markdown(binary, DOC_ID.to_string(), false, None).expect("markdown");
  assert!(result.markdown.contains("|Nom|Valeur|"), "{}", result.markdown);
  assert!(result.markdown.contains("|a|1|"), "{}", result.markdown);
}
