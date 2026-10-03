//! Shared markdown utilities for the doc_parser module

use std::collections::HashMap;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use y_octo::{Any, TextAttributes, TextDeltaOp, TextInsert};

use super::{
  super::{
    ParseError,
    block_spec::{
      BlockFlavour, BlockNode, BlockSpec, BlockType, BookmarkSpec, EmbedIframeSpec, EmbedYoutubeSpec, ImageSpec,
      TableSpec, count_tree_nodes,
    },
  },
  inline::InlineStyle,
};

const DEFAULT_CODE_LANG: &str = "plain text";
pub(crate) const MAX_MARKDOWN_CHARS: usize = 1_000_000;
pub(crate) const MAX_BLOCKS: usize = 20_000;

fn markdown_options() -> Options {
  Options::ENABLE_STRIKETHROUGH
    | Options::ENABLE_TABLES
    | Options::ENABLE_TASKLISTS
    | Options::ENABLE_HEADING_ATTRIBUTES
}

impl BlockType {
  pub fn from_heading_level(level: HeadingLevel) -> Self {
    match level {
      HeadingLevel::H1 => BlockType::H1,
      HeadingLevel::H2 => BlockType::H2,
      HeadingLevel::H3 => BlockType::H3,
      HeadingLevel::H4 => BlockType::H4,
      HeadingLevel::H5 => BlockType::H5,
      HeadingLevel::H6 => BlockType::H6,
    }
  }
}

#[derive(Debug, Clone)]
pub(super) struct MarkdownDocument {
  pub blocks: Vec<BlockNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InlineAttr {
  style: InlineStyle,
  value: Option<String>,
}

impl InlineAttr {
  fn new(style: InlineStyle) -> Self {
    Self { style, value: None }
  }

  fn color(value: String) -> Self {
    Self {
      style: InlineStyle::Color,
      value: Some(value),
    }
  }

  fn link(url: String) -> Self {
    Self {
      style: InlineStyle::Link,
      value: Some(url),
    }
  }

  fn key(&self) -> &'static str {
    self.style.key()
  }
}

#[derive(Debug, Default)]
struct InlineState {
  stack: Vec<InlineAttr>,
}

impl InlineState {
  fn push(&mut self, attr: InlineAttr) {
    self.stack.push(attr);
  }

  fn pop(&mut self, attr: InlineAttr) {
    if let Some(pos) = self.stack.iter().rposition(|item| item.key() == attr.key()) {
      self.stack.remove(pos);
    }
  }

  fn attrs(&self) -> Option<TextAttributes> {
    if self.stack.is_empty() {
      return None;
    }

    let mut attrs = TextAttributes::new();
    for attr in &self.stack {
      match attr.style {
        InlineStyle::Link => {
          if let Some(url) = attr.value.as_ref() {
            attrs.insert(attr.key().into(), Any::String(url.clone()));
          }
        }
        InlineStyle::Color => {
          if let Some(color) = attr.value.as_ref() {
            attrs.insert(attr.key().into(), Any::String(color.clone()));
          }
        }
        _ => {
          attrs.insert(attr.key().into(), Any::True);
        }
      }
    }

    Some(attrs)
  }

  fn attrs_with(&self, extra: InlineAttr) -> Option<TextAttributes> {
    let mut attrs = self.attrs().unwrap_or_default();
    let key = extra.key();
    match extra.style {
      InlineStyle::Link => {
        if let Some(url) = extra.value.as_ref() {
          attrs.insert(key.into(), Any::String(url.clone()));
        }
      }
      InlineStyle::Color => {
        if let Some(color) = extra.value.as_ref() {
          attrs.insert(key.into(), Any::String(color.clone()));
        }
      }
      _ => {
        attrs.insert(key.into(), Any::True);
      }
    }
    Some(attrs)
  }
}

#[derive(Debug)]
struct BlockDraft {
  id: Option<String>,
  flavour: BlockFlavour,
  block_type: Option<BlockType>,
  checked: Option<bool>,
  language: Option<String>,
  order: Option<i64>,
  text: Vec<TextDeltaOp>,
  children: Vec<BlockNode>,
}

impl BlockDraft {
  fn new(flavour: BlockFlavour, block_type: Option<BlockType>) -> Self {
    let block_type = if flavour == BlockFlavour::Paragraph {
      block_type.or(Some(BlockType::Text))
    } else {
      block_type
    };

    Self {
      id: None,
      flavour,
      block_type,
      checked: None,
      language: None,
      order: None,
      text: Vec::new(),
      children: Vec::new(),
    }
  }

  fn push_text(&mut self, text: &str, attrs: Option<TextAttributes>) {
    if text.is_empty() {
      return;
    }

    self.text.push(TextDeltaOp::Insert {
      insert: TextInsert::Text(text.to_string()),
      format: attrs,
    });
  }

  fn is_empty(&self) -> bool {
    self.text.is_empty() && self.children.is_empty()
  }

  fn finish(self) -> BlockNode {
    BlockNode {
      id: self.id,
      spec: BlockSpec {
        flavour: self.flavour,
        block_type: self.block_type,
        text: self.text,
        checked: self.checked,
        language: self.language,
        order: self.order,
        image: None,
        table: None,
        bookmark: None,
        embed_youtube: None,
        embed_iframe: None,
        callout_emoji: None,
      },
      children: self.children,
    }
  }
}

#[derive(Debug)]
struct ListContext {
  ordered: bool,
  next_index: i64,
}

#[derive(Debug, Clone)]
struct ImageDraft {
  source: String,
  caption: String,
  width: Option<f64>,
  height: Option<f64>,
}

impl ImageDraft {
  fn finish(self) -> Result<ImageSpec, ParseError> {
    let caption = if self.caption.trim().is_empty() {
      None
    } else {
      Some(self.caption)
    };
    let source_id = ImageSpec::normalize_source(&self.source)?;
    Ok(ImageSpec {
      source_id,
      caption,
      width: self.width,
      height: self.height,
    })
  }

  fn into_link_block(self) -> BlockNode {
    let text = if self.caption.trim().is_empty() {
      self.source.clone()
    } else {
      self.caption
    };
    let mut attrs = TextAttributes::new();
    attrs.insert("link".into(), Any::String(self.source));
    let mut draft = BlockDraft::new(BlockFlavour::Paragraph, Some(BlockType::Text));
    draft.push_text(&text, Some(attrs));
    draft.finish()
  }
}

#[derive(Debug, Default)]
struct TableState {
  rows: Vec<Vec<String>>,
  current_row: Vec<String>,
  current_cell: String,
  pending_link: Option<String>,
  pending_image: Option<ImageDraft>,
  row_in_progress: bool,
  cell_in_progress: bool,
  in_head: bool,
}

impl TableState {
  fn start_row(&mut self) {
    self.current_row.clear();
    self.row_in_progress = true;
  }

  fn finish_row(&mut self) {
    if self.row_in_progress {
      self.rows.push(std::mem::take(&mut self.current_row));
    }
    self.row_in_progress = false;
  }

  fn start_cell(&mut self) {
    self.current_cell.clear();
    self.cell_in_progress = true;
  }

  fn finish_cell(&mut self) {
    if self.cell_in_progress {
      let cell = self.current_cell.trim().to_string();
      self.current_row.push(cell);
    }
    self.current_cell.clear();
    self.cell_in_progress = false;
  }

  fn push_text(&mut self, text: &str) {
    self.current_cell.push_str(text);
  }

  fn push_marker(&mut self, marker: &str) {
    self.current_cell.push_str(marker);
  }
}

pub(crate) fn parse_markdown_blocks(markdown: &str) -> Result<Vec<BlockNode>, ParseError> {
  parse_markdown_blocks_inner(markdown, None)
}

pub(crate) fn parse_markdown_blocks_with_id_hints(
  markdown: &str,
  hint_token: &str,
) -> Result<Vec<BlockNode>, ParseError> {
  parse_markdown_blocks_inner(markdown, Some(hint_token))
}

fn parse_markdown_blocks_inner(
  markdown: &str,
  block_id_hint_token: Option<&str>,
) -> Result<Vec<BlockNode>, ParseError> {
  let normalized = normalize_markdown(markdown);
  if normalized.len() > MAX_MARKDOWN_CHARS {
    return Err(ParseError::ParserError("markdown_too_large".into()));
  }

  validate_markdown_inner(&normalized)?;
  let parsed = parse_markdown_inner(&normalized, block_id_hint_token)?;
  if count_tree_nodes(&parsed.blocks) > MAX_BLOCKS {
    return Err(ParseError::ParserError("block_count_too_large".into()));
  }
  Ok(parsed.blocks)
}

/// Parses markdown content into blocks suitable for building a ydoc.
///
/// The first H1 can be skipped to act as the document title.
fn parse_markdown_inner(markdown: &str, block_id_hint_token: Option<&str>) -> Result<MarkdownDocument, ParseError> {
  let parser = Parser::new_ext(markdown, markdown_options());

  let mut blocks: Vec<BlockNode> = Vec::new();

  let mut inline = InlineState::default();
  let mut list_stack: Vec<ListContext> = Vec::new();
  let mut list_items: Vec<BlockDraft> = Vec::new();
  let mut active: Option<BlockDraft> = None;
  let mut in_blockquote = false;
  let mut pending_image: Option<ImageDraft> = None;
  let mut pending_bookmark: Option<String> = None;
  let mut table_state: Option<TableState> = None;
  let mut span_stack: Vec<bool> = Vec::new();
  let mut aside_content: Option<String> = None;

  for event in parser {
    if aside_content.is_some() {
      match &event {
        Event::Html(html) | Event::InlineHtml(html) if is_notion_aside_close(html) => {
          let content = aside_content.take().unwrap_or_default();
          let (emoji, content) = extract_callout_emoji(&content);
          let content = content.trim();
          validate_markdown_inner(content)?;
          let children = parse_markdown_inner(content, block_id_hint_token)?.blocks;
          attach_block(callout_block(emoji, children), &mut list_items, &mut blocks);
        }
        Event::Html(html) | Event::InlineHtml(html) => {
          if let Some(content) = aside_content.as_mut() {
            content.push_str(html);
          }
        }
        Event::Text(text) | Event::Code(text) => {
          if let Some(content) = aside_content.as_mut() {
            content.push_str(text);
          }
        }
        Event::SoftBreak | Event::HardBreak => {
          if let Some(content) = aside_content.as_mut() {
            content.push('\n');
          }
        }
        _ => {}
      }
      continue;
    }

    let mut table_completed: Option<Vec<Vec<String>>> = None;
    let mut table_handled = false;
    if let Some(state) = table_state.as_mut() {
      match &event {
        Event::Start(Tag::TableHead) => {
          state.in_head = true;
          table_handled = true;
        }
        Event::End(TagEnd::TableHead) => {
          if state.cell_in_progress {
            state.finish_cell();
          }
          state.finish_row();
          state.in_head = false;
          table_handled = true;
        }
        Event::Start(Tag::TableRow) => {
          state.start_row();
          table_handled = true;
        }
        Event::End(TagEnd::TableRow) => {
          if state.cell_in_progress {
            state.finish_cell();
          }
          state.finish_row();
          table_handled = true;
        }
        Event::Start(Tag::TableCell) => {
          if state.in_head && !state.row_in_progress {
            state.start_row();
          }
          state.start_cell();
          table_handled = true;
        }
        Event::End(TagEnd::TableCell) => {
          state.finish_cell();
          table_handled = true;
        }
        Event::Start(Tag::Image { dest_url, .. }) => {
          state.pending_image = Some(ImageDraft {
            source: dest_url.to_string(),
            caption: String::new(),
            width: None,
            height: None,
          });
          table_handled = true;
        }
        Event::End(TagEnd::Image) => {
          if let Some(image) = state.pending_image.take() {
            let alt = image.caption.trim();
            let src = image.source;
            let fragment = if alt.is_empty() {
              format!("![]({src})")
            } else {
              format!("![{alt}]({src})")
            };
            state.push_text(&fragment);
          }
          table_handled = true;
        }
        Event::Text(text) => {
          if let Some(image) = state.pending_image.as_mut() {
            image.caption.push_str(text);
          } else {
            state.push_text(text);
          }
          table_handled = true;
        }
        Event::Code(code) => {
          let fragment = format!("`{code}`");
          state.push_text(&fragment);
          table_handled = true;
        }
        Event::SoftBreak | Event::HardBreak => {
          state.push_text(" ");
          table_handled = true;
        }
        Event::Start(Tag::Strong) => {
          state.push_marker("**");
          table_handled = true;
        }
        Event::End(TagEnd::Strong) => {
          state.push_marker("**");
          table_handled = true;
        }
        Event::Start(Tag::Emphasis) => {
          state.push_marker("_");
          table_handled = true;
        }
        Event::End(TagEnd::Emphasis) => {
          state.push_marker("_");
          table_handled = true;
        }
        Event::Start(Tag::Strikethrough) => {
          state.push_marker("~~");
          table_handled = true;
        }
        Event::End(TagEnd::Strikethrough) => {
          state.push_marker("~~");
          table_handled = true;
        }
        Event::Start(Tag::Link { dest_url, .. }) => {
          state.push_marker("[");
          state.pending_link = Some(dest_url.to_string());
          table_handled = true;
        }
        Event::End(TagEnd::Link) => {
          if let Some(url) = state.pending_link.take() {
            state.push_marker(&format!("]({url})"));
          }
          table_handled = true;
        }
        Event::Html(html) | Event::InlineHtml(html) => {
          if let Some(text) = extract_wrapped_html_text(html) {
            state.push_text(&text);
          } else if is_html_line_break(html) {
            state.push_text("\n");
          } else if let Some(tag) = parse_html_tag(html)
            && is_supported_text_style_tag(&tag.name)
          {
            // Ignore inline formatting tags inside table cells.
          } else if !html.trim().is_empty() {
            state.push_text(html);
          }
          table_handled = true;
        }
        Event::End(TagEnd::Table) => {
          if state.cell_in_progress {
            state.finish_cell();
          }
          state.finish_row();
          table_completed = Some(std::mem::take(&mut state.rows));
          table_handled = true;
        }
        _ => {}
      }
    }
    if let Some(rows) = table_completed {
      table_state = None;
      attach_block(table_block(rows), &mut list_items, &mut blocks);
      continue;
    }
    if table_handled {
      continue;
    }
    if matches!(event, Event::Start(Tag::Table(_))) {
      if let Some(block) = active.take() {
        attach_block(block.finish(), &mut list_items, &mut blocks);
      }
      table_state = Some(TableState::default());
      continue;
    }

    match event {
      Event::Start(Tag::Heading { level, .. }) => {
        active = Some(BlockDraft::new(
          BlockFlavour::Paragraph,
          Some(BlockType::from_heading_level(level)),
        ));
      }
      Event::End(TagEnd::Heading(_)) => {
        if let Some(block) = active.take() {
          attach_block(block.finish(), &mut list_items, &mut blocks);
        }
      }
      Event::Start(Tag::Paragraph) => {
        if in_blockquote {
          if active.is_none() {
            active = Some(BlockDraft::new(BlockFlavour::Paragraph, Some(BlockType::Quote)));
          }
        } else if list_items.is_empty() {
          active = Some(BlockDraft::new(BlockFlavour::Paragraph, Some(BlockType::Text)));
        }
      }
      Event::End(TagEnd::Paragraph) => {
        if let Some(url) = pending_bookmark.take()
          && active.as_ref().is_some_and(|block| block.is_empty())
        {
          active = None;
          attach_block(bookmark_block(url), &mut list_items, &mut blocks);
          continue;
        }
        if in_blockquote {
          if let Some(block) = active.as_mut() {
            block.push_text("\n", None);
          }
        } else if let Some(block) = active.take() {
          attach_block(block.finish(), &mut list_items, &mut blocks);
        }
      }
      Event::Start(Tag::BlockQuote(_)) => {
        in_blockquote = true;
      }
      Event::End(TagEnd::BlockQuote(_)) => {
        in_blockquote = false;
        if let Some(block) = active.take() {
          attach_block(block.finish(), &mut list_items, &mut blocks);
        }
      }
      Event::Start(Tag::List(start_num)) => {
        let start = start_num.unwrap_or(1) as i64;
        list_stack.push(ListContext {
          ordered: start_num.is_some(),
          next_index: start,
        });
      }
      Event::End(TagEnd::List(_)) => {
        list_stack.pop();
      }
      Event::Start(Tag::Item) => {
        let Some(context) = list_stack.last_mut() else {
          continue;
        };
        let order = if context.ordered {
          let order = context.next_index;
          context.next_index += 1;
          Some(order)
        } else {
          None
        };

        let block_type = if context.ordered {
          BlockType::Numbered
        } else {
          BlockType::Bulleted
        };

        let mut draft = BlockDraft::new(BlockFlavour::List, Some(block_type));
        draft.checked = None;
        draft.order = order;
        list_items.push(draft);
      }
      Event::End(TagEnd::Item) => {
        if let Some(block) = list_items.pop() {
          let finished = block.finish();
          if let Some(parent) = list_items.last_mut() {
            parent.children.push(finished);
          } else {
            blocks.push(finished);
          }
        }
      }
      Event::TaskListMarker(checked) => {
        if let Some(item) = list_items.last_mut() {
          item.checked = Some(checked);
          item.block_type = Some(BlockType::Todo);
          item.order = None;
        }
      }
      Event::Start(Tag::CodeBlock(kind)) => {
        let mut draft = BlockDraft::new(BlockFlavour::Code, None);
        match kind {
          CodeBlockKind::Fenced(lang) => {
            if !lang.is_empty() {
              draft.language = Some(lang.to_string());
            } else {
              draft.language = Some(DEFAULT_CODE_LANG.to_string());
            }
          }
          CodeBlockKind::Indented => {
            draft.language = Some(DEFAULT_CODE_LANG.to_string());
          }
        }
        active = Some(draft);
      }
      Event::End(TagEnd::CodeBlock) => {
        if let Some(block) = active.take() {
          attach_block(block.finish(), &mut list_items, &mut blocks);
        }
      }
      Event::Start(Tag::Image { dest_url, .. }) => {
        if let Some(block) = active.take()
          && !block.is_empty()
        {
          attach_block(block.finish(), &mut list_items, &mut blocks);
        }
        pending_image = Some(ImageDraft {
          source: dest_url.to_string(),
          caption: String::new(),
          width: None,
          height: None,
        });
      }
      Event::End(TagEnd::Image) => {
        if let Some(image) = pending_image.take() {
          attach_image_draft(image, &mut list_items, &mut blocks)?;
        }
      }
      Event::Text(text) => {
        if pending_bookmark.is_some() && !text.trim().is_empty() {
          pending_bookmark = None;
        }
        if let Some(image) = pending_image.as_mut() {
          image.caption.push_str(&text);
        } else if let Some(block) = active.as_mut() {
          let attrs = inline.attrs();
          block.push_text(&text, attrs);
        } else if let Some(item) = list_items.last_mut() {
          let attrs = inline.attrs();
          item.push_text(&text, attrs);
        }
      }
      Event::Html(html) | Event::InlineHtml(html) => {
        if let Some(block_id) = block_id_hint_token.and_then(|token| parse_block_id_hint(&html, token)) {
          if let Some(block) = active.as_mut() {
            block.id = Some(block_id);
          } else if let Some(item) = list_items.last_mut() {
            item.id = Some(block_id);
          } else if let Some(block) = blocks.last_mut() {
            block.id = Some(block_id);
          }
          continue;
        }
        if is_ai_editable_comment(&html) {
          continue;
        }
        if is_notion_aside_open(&html) {
          if let Some(block) = active.take()
            && !block.is_empty()
          {
            attach_block(block.finish(), &mut list_items, &mut blocks);
          }
          aside_content = Some(String::new());
          continue;
        }
        if let Some((text, attrs)) = parse_wrapped_inline_html(&html) {
          if let Some(image) = pending_image.as_mut() {
            image.caption.push_str(&text);
          } else if let Some(block) = active.as_mut() {
            block.push_text(&text, attrs);
          } else if let Some(item) = list_items.last_mut() {
            item.push_text(&text, attrs);
          }
          continue;
        }
        if handle_inline_html_tag(&html, &mut inline, &mut span_stack) {
          continue;
        }
        if let Some(image) = parse_img_tag(&html) {
          if let Some(block) = active.take()
            && !block.is_empty()
          {
            attach_block(block.finish(), &mut list_items, &mut blocks);
          }
          attach_image_draft(image, &mut list_items, &mut blocks)?;
        } else if let Some(embed) = parse_iframe_tag(&html) {
          if let Some(block) = active.take()
            && !block.is_empty()
          {
            attach_block(block.finish(), &mut list_items, &mut blocks);
          }
          match embed {
            IframeEmbed::Youtube(video_id) => {
              attach_block(embed_youtube_block(video_id), &mut list_items, &mut blocks);
            }
            IframeEmbed::Iframe(url) => {
              attach_block(embed_iframe_block(url), &mut list_items, &mut blocks);
            }
          }
        } else if is_html_line_break(&html) {
          if let Some(image) = pending_image.as_mut() {
            image.caption.push(' ');
          } else if let Some(block) = active.as_mut() {
            let attrs = inline.attrs();
            block.push_text("\n", attrs);
          } else if let Some(item) = list_items.last_mut() {
            let attrs = inline.attrs();
            item.push_text("\n", attrs);
          }
        } else if let Some(block) = active.as_mut() {
          let attrs = inline.attrs();
          block.push_text(&html, attrs);
        } else if let Some(item) = list_items.last_mut() {
          let attrs = inline.attrs();
          item.push_text(&html, attrs);
        } else if !html.trim().is_empty() {
          let mut draft = BlockDraft::new(BlockFlavour::Code, None);
          draft.language = Some("html".to_string());
          draft.push_text(&html, None);
          attach_block(draft.finish(), &mut list_items, &mut blocks);
        }
      }
      Event::Code(code) => {
        if pending_bookmark.is_some() && !code.trim().is_empty() {
          pending_bookmark = None;
        }
        if let Some(image) = pending_image.as_mut() {
          image.caption.push_str(&code);
        } else {
          let attrs = inline.attrs_with(InlineAttr::new(InlineStyle::Code));
          if let Some(block) = active.as_mut() {
            block.push_text(&code, attrs);
          } else if let Some(item) = list_items.last_mut() {
            item.push_text(&code, attrs);
          }
        }
      }
      Event::SoftBreak | Event::HardBreak => {
        if pending_bookmark.is_some() {
          pending_bookmark = None;
        }
        if let Some(image) = pending_image.as_mut() {
          image.caption.push(' ');
        } else {
          let break_text = if matches!(active.as_ref().map(|b| b.flavour), Some(BlockFlavour::Code)) {
            "\n"
          } else {
            " "
          };
          if let Some(block) = active.as_mut() {
            let attrs = inline.attrs();
            block.push_text(break_text, attrs);
          } else if let Some(item) = list_items.last_mut() {
            let attrs = inline.attrs();
            item.push_text(break_text, attrs);
          }
        }
      }
      Event::Rule => {
        let divider = BlockDraft::new(BlockFlavour::Divider, None).finish();
        attach_block(divider, &mut list_items, &mut blocks);
      }
      Event::Start(Tag::Strong) => inline.push(InlineAttr::new(InlineStyle::Bold)),
      Event::End(TagEnd::Strong) => inline.pop(InlineAttr::new(InlineStyle::Bold)),
      Event::Start(Tag::Emphasis) => inline.push(InlineAttr::new(InlineStyle::Italic)),
      Event::End(TagEnd::Emphasis) => inline.pop(InlineAttr::new(InlineStyle::Italic)),
      Event::Start(Tag::Strikethrough) => inline.push(InlineAttr::new(InlineStyle::Strike)),
      Event::End(TagEnd::Strikethrough) => inline.pop(InlineAttr::new(InlineStyle::Strike)),
      Event::Start(Tag::Link { dest_url, .. }) => {
        if let Some(url) = parse_bookmark_url(&dest_url)
          && active
            .as_ref()
            .is_some_and(|block| block.flavour == BlockFlavour::Paragraph && block.is_empty())
          && list_items.is_empty()
        {
          pending_bookmark = Some(url);
        } else {
          inline.push(InlineAttr::link(dest_url.to_string()));
        }
      }
      Event::End(TagEnd::Link) if pending_bookmark.is_none() => {
        inline.pop(InlineAttr::new(InlineStyle::Link));
      }
      _ => {}
    }
  }

  if let Some(block) = active.take() {
    attach_block(block.finish(), &mut list_items, &mut blocks);
  }
  if let Some(image) = pending_image.take() {
    attach_image_draft(image, &mut list_items, &mut blocks)?;
  }
  if let Some(mut state) = table_state.take() {
    state.finish_row();
    if !state.rows.is_empty() {
      attach_block(table_block(state.rows), &mut list_items, &mut blocks);
    }
  }

  Ok(MarkdownDocument { blocks })
}

fn validate_markdown_inner(markdown: &str) -> Result<(), ParseError> {
  let parser = Parser::new_ext(markdown, markdown_options());
  let mut in_notion_aside = false;
  let mut in_custom_text_html = false;

  for event in parser {
    match event {
      Event::Start(tag) => ensure_supported_tag(&tag)?,
      Event::Html(html) | Event::InlineHtml(html) => {
        if is_notion_aside_open(&html) {
          in_notion_aside = true;
          continue;
        }
        if in_notion_aside {
          if is_notion_aside_close(&html) {
            in_notion_aside = false;
          }
          continue;
        }
        if is_supported_custom_text_html_open(&html) {
          in_custom_text_html = true;
          continue;
        }
        if in_custom_text_html {
          if is_supported_custom_text_html_close(&html) {
            in_custom_text_html = false;
          }
          continue;
        }
        if is_ai_editable_comment(&html) {
          continue;
        }
        if parse_img_tag(&html).is_some() {
          continue;
        }
        if parse_iframe_tag(&html).is_some() {
          continue;
        }
        if is_html_line_break(&html) {
          continue;
        }
        if is_supported_inline_html(&html) {
          continue;
        }
        if is_supported_custom_text_html_open(&html) || is_supported_custom_text_html_close(&html) {
          continue;
        }
        if !is_dangerous_html(&html) {
          continue;
        }
        let preview = html.chars().take(120).collect::<String>();
        return Err(ParseError::ParserError(format!("unsupported_markdown:html:{preview}")));
      }
      Event::FootnoteReference(_) => {
        return Err(ParseError::ParserError("unsupported_markdown:footnote".into()));
      }
      Event::InlineMath(_) | Event::DisplayMath(_) => {
        return Err(ParseError::ParserError("unsupported_markdown:math".into()));
      }
      _ => {}
    }
  }

  Ok(())
}

fn ensure_supported_tag(tag: &Tag) -> Result<(), ParseError> {
  match tag {
    Tag::Paragraph
    | Tag::Heading { .. }
    | Tag::BlockQuote(_)
    | Tag::CodeBlock(_)
    | Tag::List(_)
    | Tag::Item
    | Tag::Emphasis
    | Tag::Strong
    | Tag::Strikethrough
    | Tag::Link { .. }
    | Tag::Image { .. }
    | Tag::Table(_)
    | Tag::TableHead
    | Tag::TableRow
    | Tag::TableCell => Ok(()),
    Tag::HtmlBlock => Ok(()),
    Tag::FootnoteDefinition(_) => Err(ParseError::ParserError("unsupported_markdown:footnote".into())),
    Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
      Err(ParseError::ParserError("unsupported_markdown:definition_list".into()))
    }
    Tag::Superscript => Err(ParseError::ParserError("unsupported_markdown:superscript".into())),
    Tag::Subscript => Err(ParseError::ParserError("unsupported_markdown:subscript".into())),
    Tag::MetadataBlock(_) => Err(ParseError::ParserError("unsupported_markdown:metadata".into())),
  }
}

fn parse_img_tag(html: &str) -> Option<ImageDraft> {
  let tag = html.trim();
  if !tag.starts_with("<img") {
    return None;
  }
  let attrs = parse_html_attrs(tag);
  let source = attrs.get("src")?.to_string();
  let caption = attrs
    .get("alt")
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
  let width = attrs.get("width").and_then(|value| value.parse::<f64>().ok());
  let height = attrs.get("height").and_then(|value| value.parse::<f64>().ok());
  Some(ImageDraft {
    source,
    caption: caption.unwrap_or_default(),
    width,
    height,
  })
}

enum IframeEmbed {
  Youtube(String),
  Iframe(String),
}

fn parse_iframe_tag(html: &str) -> Option<IframeEmbed> {
  let tag = html.trim();
  if !tag.to_ascii_lowercase().starts_with("<iframe") {
    return None;
  }
  let attrs = parse_html_attrs(tag);
  let src = attrs.get("src")?.trim();
  if src.is_empty() {
    return None;
  }
  if let Some(video_id) = parse_youtube_video_id(src) {
    return Some(IframeEmbed::Youtube(video_id));
  }
  if is_valid_generic_embed_url(src) {
    return Some(IframeEmbed::Iframe(src.to_string()));
  }
  None
}

fn parse_youtube_video_id(src: &str) -> Option<String> {
  let src = src.trim();
  let prefix = "https://www.youtube.com/embed/";
  if !src.starts_with(prefix) {
    return None;
  }
  let id = &src[prefix.len()..];
  let id = id.split(['?', '#', '/']).next().unwrap_or("");
  if id.is_empty() { None } else { Some(id.to_string()) }
}

const AFFINE_DOMAINS: [&str; 6] = [
  "affine.pro",
  "app.affine.pro",
  "insider.affine.pro",
  "affine.fail",
  "toeverything.app",
  "apple.getaffineapp.com",
];

fn is_valid_generic_embed_url(url: &str) -> bool {
  let Some(host) = parse_https_host(url) else {
    return false;
  };
  !AFFINE_DOMAINS
    .iter()
    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn parse_https_host(url: &str) -> Option<String> {
  let trimmed = url.trim();
  if trimmed.is_empty() {
    return None;
  }
  let lower = trimmed.to_ascii_lowercase();
  let (scheme, rest) = lower.split_once("://")?;
  if scheme != "https" {
    return None;
  }
  let host_port = rest.split(&['/', '?', '#'][..]).next().unwrap_or("");
  if host_port.is_empty() {
    return None;
  }
  let host = host_port.split('@').next_back().unwrap_or("");
  let host = host.split(':').next().unwrap_or("");
  if host.is_empty() {
    return None;
  }
  Some(host.to_string())
}

fn is_ai_editable_comment(html: &str) -> bool {
  let trimmed = html.trim();
  if !trimmed.starts_with("<!--") || !trimmed.ends_with("-->") {
    return false;
  }
  let body = trimmed.trim_start_matches("<!--").trim_end_matches("-->").trim();
  body.contains("block_id=") && body.contains("flavour=")
}

fn parse_block_id_hint(html: &str, hint_token: &str) -> Option<String> {
  let body = html.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim();
  let id = body.strip_prefix(&format!("affine:block-id:{hint_token}="))?.trim();
  (!id.is_empty() && !id.chars().any(char::is_whitespace)).then(|| id.to_string())
}

fn is_html_line_break(html: &str) -> bool {
  let trimmed = html.trim();
  if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
    return false;
  }
  let inner = trimmed.trim_start_matches('<').trim_end_matches('>').trim();
  let inner = inner.trim_end_matches('/').trim();
  inner.eq_ignore_ascii_case("br")
}

fn parse_bookmark_url(dest_url: &str) -> Option<String> {
  let (prefix, url) = dest_url.split_once(',')?;
  if !prefix.trim().eq_ignore_ascii_case("bookmark") {
    return None;
  }
  let url = url.trim();
  if url.is_empty() { None } else { Some(url.to_string()) }
}

fn normalize_markdown(markdown: &str) -> String {
  if !markdown.contains('<') {
    return markdown.to_string();
  }
  normalize_html_lists(markdown)
}

#[derive(Debug, Clone, Copy)]
enum ListKind {
  Unordered,
  Ordered,
}

#[derive(Debug, Clone, Copy)]
struct ListState {
  kind: ListKind,
  counter: usize,
}

fn normalize_html_lists(markdown: &str) -> String {
  if !markdown.contains("<li") && !markdown.contains("<ul") && !markdown.contains("<ol") {
    return markdown.to_string();
  }

  let mut out = String::with_capacity(markdown.len());
  let mut in_fence = false;
  let mut fence_marker: Option<String> = None;
  let mut list_stack: Vec<ListState> = Vec::new();

  for chunk in markdown.split_inclusive('\n') {
    let line = chunk.strip_suffix('\n').unwrap_or(chunk);
    let newline = if chunk.ends_with('\n') { "\n" } else { "" };
    let trimmed = line.trim_start();

    if let Some(marker) = fence_marker_start(trimmed) {
      if !in_fence {
        in_fence = true;
        fence_marker = Some(marker.to_string());
      } else if fence_marker.as_deref() == Some(marker) {
        in_fence = false;
        fence_marker = None;
      }
      out.push_str(line);
      out.push_str(newline);
      continue;
    }

    if in_fence {
      out.push_str(line);
      out.push_str(newline);
      continue;
    }

    let normalized_line = normalize_html_lists_line(line, &mut list_stack);
    out.push_str(&normalized_line);
    out.push_str(newline);
  }

  out
}

fn fence_marker_start(line: &str) -> Option<&'static str> {
  if line.starts_with("```") {
    Some("```")
  } else if line.starts_with("~~~") {
    Some("~~~")
  } else {
    None
  }
}

fn normalize_html_lists_line(line: &str, list_stack: &mut Vec<ListState>) -> String {
  let mut out = String::with_capacity(line.len());
  let bytes = line.as_bytes();
  let mut i = 0;

  while i < line.len() {
    if bytes[i] == b'<'
      && let Some(rel_end) = line[i..].find('>')
    {
      let end = i + rel_end;
      let tag = &line[i..=end];
      if let Some(tag_info) = parse_html_tag(tag) {
        match tag_info.name.as_str() {
          "ul" => {
            if tag_info.closing {
              list_stack.pop();
            } else if !tag_info.self_closing {
              list_stack.push(ListState {
                kind: ListKind::Unordered,
                counter: 0,
              });
            }
          }
          "ol" => {
            if tag_info.closing {
              list_stack.pop();
            } else if !tag_info.self_closing {
              list_stack.push(ListState {
                kind: ListKind::Ordered,
                counter: 0,
              });
            }
          }
          "li" => {
            if tag_info.closing {
              if !out.ends_with('\n') {
                out.push('\n');
              }
            } else {
              if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
              }
              let depth = list_stack.len().saturating_sub(1);
              if depth > 0 {
                out.push_str(&"  ".repeat(depth));
              }
              let prefix = match list_stack.last_mut() {
                Some(state) => match state.kind {
                  ListKind::Unordered => "- ".to_string(),
                  ListKind::Ordered => {
                    state.counter += 1;
                    format!("{}. ", state.counter)
                  }
                },
                None => "- ".to_string(),
              };
              out.push_str(&prefix);
            }
          }
          _ => out.push_str(tag),
        }
        i = end + 1;
        continue;
      }
    }

    let ch = line[i..].chars().next().unwrap();
    out.push(ch);
    i += ch.len_utf8();
  }

  out
}

#[derive(Debug, Clone)]
struct HtmlTag {
  name: String,
  closing: bool,
  self_closing: bool,
  attrs: HashMap<String, String>,
}

fn parse_html_tag(html: &str) -> Option<HtmlTag> {
  let trimmed = html.trim();
  if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
    return None;
  }
  if trimmed.starts_with("<!--") {
    return None;
  }
  let mut inner = trimmed.trim_start_matches('<').trim_end_matches('>').trim();
  let closing = inner.starts_with('/');
  inner = inner.trim_start_matches('/');
  let self_closing = inner.ends_with('/');
  inner = inner.trim_end_matches('/').trim();
  let name_end = inner
    .char_indices()
    .find_map(|(index, ch)| (!ch.is_ascii_alphanumeric() && ch != '-' && ch != ':').then_some(index))
    .unwrap_or(inner.len());
  let name = inner[..name_end].to_lowercase();
  if name.is_empty() {
    return None;
  }
  let attrs = if closing {
    HashMap::new()
  } else {
    parse_html_attrs(trimmed)
  };
  Some(HtmlTag {
    name,
    closing,
    self_closing,
    attrs,
  })
}

fn parse_style_color(style: &str) -> Option<String> {
  for part in style.split(';') {
    let (key, value) = part.split_once(':')?;
    if key.trim().eq_ignore_ascii_case("color") {
      let color = value.trim();
      if !color.is_empty() {
        return Some(color.to_string());
      }
    }
  }
  None
}

fn parse_wrapped_inline_html(html: &str) -> Option<(String, Option<TextAttributes>)> {
  let wrapped = parse_wrapped_html_text(html)?;
  match wrapped.name.as_str() {
    "u" => {
      let mut attrs = TextAttributes::new();
      attrs.insert(InlineStyle::Underline.key().into(), Any::True);
      Some((wrapped.text, Some(attrs)))
    }
    "strong" | "b" => {
      let mut attrs = TextAttributes::new();
      attrs.insert(InlineStyle::Bold.key().into(), Any::True);
      Some((wrapped.text, Some(attrs)))
    }
    "em" | "i" => {
      let mut attrs = TextAttributes::new();
      attrs.insert(InlineStyle::Italic.key().into(), Any::True);
      Some((wrapped.text, Some(attrs)))
    }
    "del" | "s" | "strike" => {
      let mut attrs = TextAttributes::new();
      attrs.insert(InlineStyle::Strike.key().into(), Any::True);
      Some((wrapped.text, Some(attrs)))
    }
    "a" => {
      let attrs = wrapped.attrs.get("href").map(|href| {
        let mut attrs = TextAttributes::new();
        attrs.insert(InlineStyle::Link.key().into(), Any::String(href.clone()));
        attrs
      });
      Some((wrapped.text, attrs))
    }
    "span" => {
      let color = wrapped.attrs.get("style").and_then(|style| parse_style_color(style));
      let attrs = color.map(|color| {
        let mut attrs = TextAttributes::new();
        attrs.insert(InlineStyle::Color.key().into(), Any::String(color));
        attrs
      });
      Some((wrapped.text, attrs))
    }
    _ => None,
  }
}

fn extract_wrapped_html_text(html: &str) -> Option<String> {
  parse_wrapped_html_text(html).map(|wrapped| wrapped.text)
}

fn parse_wrapped_html_text(html: &str) -> Option<WrappedHtmlText> {
  let trimmed = html.trim();
  if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
    return None;
  }
  let open_end = trimmed.find('>')?;
  let open_tag = &trimmed[..=open_end];
  let open_info = parse_html_tag(open_tag)?;
  if open_info.closing {
    return None;
  }
  let close_tag = format!("</{}>", open_info.name);
  if !trimmed.ends_with(&close_tag) {
    return None;
  }
  let inner_start = open_end + 1;
  let inner_end = trimmed.len().saturating_sub(close_tag.len());
  if inner_start >= inner_end {
    return None;
  }
  let inner = &trimmed[inner_start..inner_end];
  if inner.contains('<') {
    return None;
  }
  Some(WrappedHtmlText {
    name: open_info.name,
    attrs: open_info.attrs,
    text: inner.to_string(),
  })
}

#[derive(Debug, Clone)]
struct WrappedHtmlText {
  name: String,
  attrs: HashMap<String, String>,
  text: String,
}

fn handle_inline_html_tag(html: &str, inline: &mut InlineState, span_stack: &mut Vec<bool>) -> bool {
  let Some(tag) = parse_html_tag(html) else {
    return false;
  };

  match tag.name.as_str() {
    "strong" | "b" => {
      if tag.closing {
        inline.pop(InlineAttr::new(InlineStyle::Bold));
      } else if !tag.self_closing {
        inline.push(InlineAttr::new(InlineStyle::Bold));
      }
      true
    }
    "em" | "i" => {
      if tag.closing {
        inline.pop(InlineAttr::new(InlineStyle::Italic));
      } else if !tag.self_closing {
        inline.push(InlineAttr::new(InlineStyle::Italic));
      }
      true
    }
    "del" | "s" | "strike" => {
      if tag.closing {
        inline.pop(InlineAttr::new(InlineStyle::Strike));
      } else if !tag.self_closing {
        inline.push(InlineAttr::new(InlineStyle::Strike));
      }
      true
    }
    "a" => {
      if tag.closing {
        inline.pop(InlineAttr::link(String::new()));
      } else if !tag.self_closing
        && let Some(href) = tag.attrs.get("href")
      {
        inline.push(InlineAttr::link(href.clone()));
      }
      true
    }
    "u" => {
      if tag.closing {
        inline.pop(InlineAttr::new(InlineStyle::Underline));
      } else if !tag.self_closing {
        inline.push(InlineAttr::new(InlineStyle::Underline));
      }
      true
    }
    "span" => {
      if tag.closing {
        if span_stack.pop().unwrap_or(false) {
          inline.pop(InlineAttr::color(String::new()));
        }
      } else if !tag.self_closing {
        let color = tag.attrs.get("style").and_then(|style| parse_style_color(style));
        if let Some(color) = color {
          inline.push(InlineAttr::color(color));
          span_stack.push(true);
        } else {
          span_stack.push(false);
        }
      }
      true
    }
    "ul" | "ol" | "li" => true,
    _ => false,
  }
}

fn is_supported_inline_html(html: &str) -> bool {
  let Some(tag) = parse_html_tag(html) else {
    return false;
  };
  is_supported_text_style_tag(&tag.name) || matches!(tag.name.as_str(), "ul" | "ol" | "li" | "aside")
}

fn is_supported_text_style_tag(name: &str) -> bool {
  matches!(
    name,
    "strong" | "b" | "em" | "i" | "del" | "s" | "strike" | "a" | "u" | "span"
  )
}

fn is_supported_custom_text_html_open(html: &str) -> bool {
  let Some(tag) = parse_html_tag(html) else {
    return false;
  };
  tag.name == "type" && tag.attrs.is_empty() && !html.trim().starts_with("</")
}

fn is_supported_custom_text_html_close(html: &str) -> bool {
  html.trim().eq_ignore_ascii_case("</type>")
}

fn is_dangerous_html(html: &str) -> bool {
  let Some(tag) = parse_html_tag(html) else {
    return false;
  };
  matches!(
    tag.name.as_str(),
    "script" | "style" | "iframe" | "object" | "embed" | "link" | "meta"
  )
}

fn is_notion_aside_open(html: &str) -> bool {
  let trimmed = html.trim();
  trimmed.starts_with("<aside") && trimmed.ends_with('>')
}

fn is_notion_aside_close(html: &str) -> bool {
  html.trim() == "</aside>"
}

fn parse_html_attrs(tag: &str) -> HashMap<String, String> {
  let mut attrs = HashMap::new();
  let chars: Vec<char> = tag.chars().collect();
  let mut i = match tag.find('<') {
    Some(pos) => pos + 1,
    None => return attrs,
  };
  while i < chars.len() && chars[i].is_whitespace() {
    i += 1;
  }
  while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
    i += 1;
  }
  while i < chars.len() {
    while i < chars.len() && chars[i].is_whitespace() {
      i += 1;
    }
    if i >= chars.len() || chars[i] == '>' {
      break;
    }
    if chars[i] == '/' {
      i += 1;
      continue;
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != '>' {
      i += 1;
    }
    let key: String = chars[start..i].iter().collect::<String>().to_lowercase();
    while i < chars.len() && chars[i].is_whitespace() {
      i += 1;
    }
    if i >= chars.len() || chars[i] != '=' {
      continue;
    }
    i += 1;
    while i < chars.len() && chars[i].is_whitespace() {
      i += 1;
    }
    if i >= chars.len() {
      break;
    }
    let value = if chars[i] == '"' || chars[i] == '\'' {
      let quote = chars[i];
      i += 1;
      let start = i;
      while i < chars.len() && chars[i] != quote {
        i += 1;
      }
      let value: String = chars[start..i].iter().collect();
      if i < chars.len() && chars[i] == quote {
        i += 1;
      }
      value
    } else {
      let start = i;
      while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
        i += 1;
      }
      chars[start..i].iter().collect()
    };
    if !key.is_empty() && !value.is_empty() {
      attrs.insert(key, value);
    }
  }
  attrs
}

fn image_block(image: ImageSpec) -> BlockNode {
  BlockNode {
    id: None,
    spec: BlockSpec {
      flavour: BlockFlavour::Image,
      block_type: None,
      text: Vec::new(),
      checked: None,
      language: None,
      order: None,
      image: Some(image),
      table: None,
      bookmark: None,
      embed_youtube: None,
      embed_iframe: None,
      callout_emoji: None,
    },
    children: Vec::new(),
  }
}

fn embed_youtube_block(video_id: String) -> BlockNode {
  BlockNode {
    id: None,
    spec: BlockSpec {
      flavour: BlockFlavour::EmbedYoutube,
      block_type: None,
      text: Vec::new(),
      checked: None,
      language: None,
      order: None,
      image: None,
      table: None,
      bookmark: None,
      embed_youtube: Some(EmbedYoutubeSpec { video_id }),
      embed_iframe: None,
      callout_emoji: None,
    },
    children: Vec::new(),
  }
}

fn embed_iframe_block(url: String) -> BlockNode {
  BlockNode {
    id: None,
    spec: BlockSpec {
      flavour: BlockFlavour::EmbedIframe,
      block_type: None,
      text: Vec::new(),
      checked: None,
      language: None,
      order: None,
      image: None,
      table: None,
      bookmark: None,
      embed_youtube: None,
      embed_iframe: Some(EmbedIframeSpec { url }),
      callout_emoji: None,
    },
    children: Vec::new(),
  }
}

fn bookmark_block(url: String) -> BlockNode {
  BlockNode {
    id: None,
    spec: BlockSpec {
      flavour: BlockFlavour::Bookmark,
      block_type: None,
      text: Vec::new(),
      checked: None,
      language: None,
      order: None,
      image: None,
      table: None,
      bookmark: Some(BookmarkSpec { url, caption: None }),
      embed_youtube: None,
      embed_iframe: None,
      callout_emoji: None,
    },
    children: Vec::new(),
  }
}

fn table_block(rows: Vec<Vec<String>>) -> BlockNode {
  BlockNode {
    id: None,
    spec: BlockSpec {
      flavour: BlockFlavour::Table,
      block_type: None,
      text: Vec::new(),
      checked: None,
      language: None,
      order: None,
      image: None,
      table: Some(TableSpec { rows }),
      bookmark: None,
      embed_youtube: None,
      embed_iframe: None,
      callout_emoji: None,
    },
    children: Vec::new(),
  }
}

fn callout_block(emoji: Option<String>, children: Vec<BlockNode>) -> BlockNode {
  BlockNode {
    id: None,
    spec: BlockSpec {
      flavour: BlockFlavour::Callout,
      block_type: None,
      text: Vec::new(),
      checked: None,
      language: None,
      order: None,
      image: None,
      table: None,
      bookmark: None,
      embed_youtube: None,
      embed_iframe: None,
      callout_emoji: emoji,
    },
    children,
  }
}

fn extract_callout_emoji(content: &str) -> (Option<String>, &str) {
  let trimmed = content.trim_start();
  let Some(first) = trimmed.chars().next() else {
    return (None, trimmed);
  };
  if is_emoji(first) {
    let rest = trimmed[first.len_utf8()..].trim_start();
    (Some(first.to_string()), rest)
  } else {
    (None, trimmed)
  }
}

fn is_emoji(ch: char) -> bool {
  matches!(ch as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF)
}

fn attach_block(block: BlockNode, list_items: &mut [BlockDraft], blocks: &mut Vec<BlockNode>) {
  if let Some(parent) = list_items.last_mut() {
    parent.children.push(block);
  } else {
    blocks.push(block);
  }
}

fn attach_image_draft(
  image: ImageDraft,
  list_items: &mut [BlockDraft],
  blocks: &mut Vec<BlockNode>,
) -> Result<(), ParseError> {
  match image.clone().finish() {
    Ok(image) => attach_block(image_block(image), list_items, blocks),
    Err(ParseError::ParserError(reason)) if reason.starts_with("unsupported_image_source") => {
      attach_block(image.into_link_block(), list_items, blocks);
    }
    Err(error) => return Err(error),
  }
  Ok(())
}

#[cfg(test)]
#[path = "../tests/markdown/parser.rs"]
mod tests;
