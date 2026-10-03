pub(super) const PAGE_FLAVOUR: &str = "affine:page";
pub(super) const NOTE_FLAVOUR: &str = "affine:note";
pub(super) const SURFACE_FLAVOUR: &str = "affine:surface";

pub(super) const SYS_ID: &str = "sys:id";
pub(super) const SYS_FLAVOUR: &str = "sys:flavour";
pub(super) const SYS_VERSION: &str = "sys:version";
pub(super) const SYS_CHILDREN: &str = "sys:children";

pub(super) const PROP_TITLE: &str = "prop:title";
pub(super) const PROP_TEXT: &str = "prop:text";
pub(super) const PROP_TYPE: &str = "prop:type";
pub(super) const PROP_CHECKED: &str = "prop:checked";
pub(super) const PROP_LANGUAGE: &str = "prop:language";
pub(super) const PROP_ORDER: &str = "prop:order";

pub(super) const PROP_ELEMENTS: &str = "prop:elements";
pub(super) const PROP_BACKGROUND: &str = "prop:background";
pub(super) const PROP_XYWH: &str = "prop:xywh";
pub(super) const PROP_INDEX: &str = "prop:index";
pub(super) const PROP_HIDDEN: &str = "prop:hidden";
pub(super) const PROP_DISPLAY_MODE: &str = "prop:displayMode";

pub(super) const PROP_SOURCE_ID: &str = "prop:sourceId";
pub(super) const PROP_CAPTION: &str = "prop:caption";
pub(super) const PROP_WIDTH: &str = "prop:width";
pub(super) const PROP_HEIGHT: &str = "prop:height";
pub(super) const PROP_URL: &str = "prop:url";
pub(super) const PROP_VIDEO_ID: &str = "prop:videoId";

pub(super) const PROP_ROWS_PREFIX: &str = "prop:rows.";
pub(super) const PROP_COLUMNS_PREFIX: &str = "prop:columns.";
pub(super) const PROP_CELLS_PREFIX: &str = "prop:cells.";
pub(super) const PROP_ROW_ID_SUFFIX: &str = ".rowId";
pub(super) const PROP_COLUMN_ID_SUFFIX: &str = ".columnId";
pub(super) const PROP_ORDER_SUFFIX: &str = ".order";
pub(super) const PROP_TEXT_SUFFIX: &str = ".text";

pub(super) fn table_row_id_key(row_id: &str) -> String {
  format!("{PROP_ROWS_PREFIX}{row_id}{PROP_ROW_ID_SUFFIX}")
}

pub(super) fn table_row_order_key(row_id: &str) -> String {
  format!("{PROP_ROWS_PREFIX}{row_id}{PROP_ORDER_SUFFIX}")
}

pub(super) fn table_column_id_key(column_id: &str) -> String {
  format!("{PROP_COLUMNS_PREFIX}{column_id}{PROP_COLUMN_ID_SUFFIX}")
}

pub(super) fn table_column_order_key(column_id: &str) -> String {
  format!("{PROP_COLUMNS_PREFIX}{column_id}{PROP_ORDER_SUFFIX}")
}

const ORDER_KEY_DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Order key of the `index`-th table row or column, in the fractional-indexing
/// format used by the editor (`a0`..`az`, then `b00`..`bzz`, ...). Keys sort in
/// index order and are accepted by `generateKeyBetween` when rows or columns
/// are inserted later.
pub(super) fn table_order_key(index: usize) -> String {
  let base = ORDER_KEY_DIGITS.len();
  let mut remaining = index;
  let mut width = 1;
  let mut capacity = base;
  while remaining >= capacity {
    remaining -= capacity;
    width += 1;
    capacity *= base;
  }
  let mut digits = vec![b'0'; width];
  for slot in digits.iter_mut().rev() {
    *slot = ORDER_KEY_DIGITS[remaining % base];
    remaining /= base;
  }
  let head = (b'a' + (width - 1) as u8) as char;
  format!("{head}{}", String::from_utf8(digits).expect("ascii digits"))
}

pub(super) fn table_cell_text_key(row_id: &str, column_id: &str) -> String {
  format!("{PROP_CELLS_PREFIX}{row_id}:{column_id}{PROP_TEXT_SUFFIX}")
}
