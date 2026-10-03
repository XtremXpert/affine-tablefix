fix(doc_loader): write table cells as Y.Text with fractional order keys

Fixes #15466.

## Problem

Tables written from markdown by `build_full_doc` / `update_doc` (MCP `create_document`
and `update_document`) are stored with:

- each cell as a plain string (`Any::String`) under `prop:cells.<row>:<col>.text`, while
  the editor binds cells to a `Y.Text` — the grid renders with the right number of cells,
  all empty, although `read_document` still returns the text;
- row and column orders as zero-padded integers (`000000`, `000001`, …), which are not
  fractional-indexing keys — inserting a column in the editor throws
  `invalid order key head: 0`.

## Fix

- `apply_table_block_props` now takes the `Doc` and writes every cell with the existing
  `insert_text` helper (same path as paragraph text).
- New `table_order_key(index)` in `schema.rs` generates fractional-indexing keys in the
  editor's format (`a0`…`az`, then `b00`…), used for rows and columns in both the y-octo
  writer and the JSON snapshot (`table_props` in `lib.rs`).

## Tests

`tests/table_cells.rs` (integration test, public API only):

- cells of a created table are `Y.Text` with the expected content;
- orders are valid keys and sort in markdown order (100 rows, across the `az` → `b00` boundary);
- tables added through `update_doc` get the same structure;
- markdown export still round-trips the table.

All four fail on 0.1.9 and pass with the patch. Verified on a self-hosted 0.27.4 (crate
0.1.7, same code) by rebuilding `server-native.x64.node`.

Note: tables already written by earlier versions stay broken; `update_doc` keeps
`is_exact` table blocks, so they must be rewritten (e.g. delete and re-add the table).
