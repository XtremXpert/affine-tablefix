mod delta;
mod inline;
mod parser;
mod render;

pub(crate) use delta::{
  DeltaToMdOptions, InlineReferencePayload, delta_value_to_inline_markdown, extract_inline_references,
  extract_inline_references_from_value, text_to_inline_markdown,
};
#[cfg(test)]
pub(crate) use parser::MAX_MARKDOWN_CHARS;
pub(crate) use parser::{MAX_BLOCKS, parse_markdown_blocks, parse_markdown_blocks_with_id_hints};
pub(crate) use render::{MarkdownRenderer, MarkdownWriter};

/// Inline delta of a table cell. The parser stores cells as inline markdown
/// (bold, code, links are re-serialized), so a cell is parsed like a paragraph
/// to keep its formatting; anything else (a list marker, a heading...) stays
/// plain text.
pub(crate) fn table_cell_ops(cell: &str) -> Vec<y_octo::TextDeltaOp> {
  use crate::block_spec::{BlockFlavour, BlockType};

  if let Ok(blocks) = parse_markdown_blocks(cell)
    && let [block] = blocks.as_slice()
    && block.children.is_empty()
    && block.spec.flavour == BlockFlavour::Paragraph
    && matches!(block.spec.block_type, None | Some(BlockType::Text))
  {
    return block.spec.text.clone();
  }
  if cell.is_empty() {
    Vec::new()
  } else {
    vec![y_octo::TextDeltaOp::Insert {
      insert: y_octo::TextInsert::Text(cell.to_string()),
      format: None,
    }]
  }
}
