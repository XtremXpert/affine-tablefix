use std::collections::{HashMap, HashSet};

use super::{
  schema::{SYS_CHILDREN, SYS_FLAVOUR, SYS_ID},
  value::{ArrayRead, MapRead, ValueRead, value_to_string},
};

pub(super) struct BlockIndex<M: MapRead> {
  pub(super) block_pool: HashMap<String, M>,
  pub(super) parent_lookup: HashMap<String, String>,
}

pub(super) struct DocContext<M: MapRead> {
  pub(super) block_pool: HashMap<String, M>,
  pub(super) parent_lookup: HashMap<String, String>,
  pub(super) root_block_id: String,
}

pub(super) struct BlockWalker {
  queue: Vec<(Option<String>, String)>,
  visited: HashSet<String>,
}

pub(super) fn build_block_index<M: MapRead>(blocks_map: &M) -> BlockIndex<M> {
  let mut block_pool = HashMap::new();
  let mut parent_lookup = HashMap::new();

  for value in blocks_map.values() {
    if let Some(block_map) = value.as_map_view()
      && let Some(block_id) = get_block_id(&block_map)
    {
      for child_id in collect_child_ids(&block_map) {
        parent_lookup.insert(child_id, block_id.clone());
      }
      block_pool.insert(block_id, block_map);
    }
  }

  BlockIndex {
    block_pool,
    parent_lookup,
  }
}

impl<M: MapRead> DocContext<M> {
  pub(super) fn from_blocks_map(blocks_map: &M, root_flavour: &str) -> Option<Self> {
    let BlockIndex {
      block_pool,
      parent_lookup,
    } = build_block_index(blocks_map);

    let root_block_id = find_block_id_by_flavour(&block_pool, root_flavour)?;
    Some(Self {
      block_pool,
      parent_lookup,
      root_block_id,
    })
  }

  pub(super) fn walker(&self) -> BlockWalker {
    BlockWalker::new(&self.root_block_id)
  }
}

impl BlockWalker {
  fn new(root_block_id: &str) -> Self {
    let mut visited = HashSet::new();
    visited.insert(root_block_id.to_string());

    Self {
      queue: vec![(None, root_block_id.to_string())],
      visited,
    }
  }

  pub(super) fn next(&mut self) -> Option<(Option<String>, String)> {
    self.queue.pop()
  }

  pub(super) fn enqueue_children<M: MapRead>(&mut self, parent_block_id: &str, block: &M) {
    let mut child_ids = collect_child_ids(block);
    for child_id in child_ids.drain(..).rev() {
      if self.visited.insert(child_id.clone()) {
        self.queue.push((Some(parent_block_id.to_string()), child_id));
      }
    }
  }
}

pub(super) fn find_block_id_by_flavour<M: MapRead>(block_pool: &HashMap<String, M>, flavour: &str) -> Option<String> {
  block_pool.iter().find_map(|(id, block)| {
    get_flavour(block)
      .filter(|block_flavour| block_flavour == flavour)
      .map(|_| id.clone())
  })
}

pub(super) fn collect_child_ids<M: MapRead>(block: &M) -> Vec<String> {
  block
    .get_value(SYS_CHILDREN)
    .and_then(|value| value.as_array_view())
    .map(|array| {
      array
        .items()
        .filter_map(|value| value_to_string(&value))
        .collect::<Vec<_>>()
    })
    .unwrap_or_default()
}

pub(super) fn get_block_id<M: MapRead>(block: &M) -> Option<String> {
  get_string(block, SYS_ID)
}

pub(super) fn get_flavour<M: MapRead>(block: &M) -> Option<String> {
  get_string(block, SYS_FLAVOUR)
}

pub(super) fn get_string<M: MapRead>(block: &M, key: &str) -> Option<String> {
  block.get_value(key).and_then(|value| value_to_string(&value))
}

pub(super) fn get_list_depth<M: MapRead>(
  block_id: &str,
  parent_lookup: &HashMap<String, String>,
  blocks: &HashMap<String, M>,
) -> usize {
  let mut depth = 0;
  let mut current_id = block_id.to_string();

  while let Some(parent_id) = parent_lookup.get(&current_id) {
    if let Some(parent_block) = blocks.get(parent_id)
      && get_flavour(parent_block).as_deref() == Some("affine:list")
    {
      depth += 1;
      current_id = parent_id.clone();
      continue;
    }
    break;
  }
  depth
}

pub(super) fn nearest_by_flavour<M: MapRead>(
  start: &str,
  flavour: &str,
  parent_lookup: &HashMap<String, String>,
  blocks: &HashMap<String, M>,
) -> Option<M> {
  let mut cursor = Some(start.to_string());
  while let Some(node) = cursor {
    if let Some(block) = blocks.get(&node)
      && get_flavour(block).as_deref() == Some(flavour)
    {
      return Some(block.clone());
    }
    cursor = parent_lookup.get(&node).cloned();
  }
  None
}

pub(super) fn find_child_id_by_flavour<M: MapRead>(
  parent: &M,
  block_pool: &HashMap<String, M>,
  flavour: &str,
) -> Option<String> {
  collect_child_ids(parent).into_iter().find(|id| {
    block_pool
      .get(id)
      .and_then(get_flavour)
      .as_deref()
      .is_some_and(|value| value == flavour)
  })
}
