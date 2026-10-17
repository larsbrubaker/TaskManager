//! The parent/child process forest (PLAN §5.3 "expandable children") and the lookup
//! index behind [`ProcessTable`]'s queries.
//!
//! [`TableIndex`] is built once per table, on first lookup: key → row, live pid → row,
//! and each row's parent and children. Parents are resolved defensively, since OS data
//! is racy:
//! - a parent pid with no row (parent exited, or hidden from us) makes the child a root;
//! - a recycled parent pid is detected by start time (a parent cannot have started after
//!   its child), and the child becomes a root rather than adopting a stranger; when an
//!   exited row and a live row share the pid, the one that started most recently but no
//!   later than the child wins;
//! - cycles (pid 0 naming itself, or racy reads producing a ↔ b) are broken by making the
//!   cycle's smallest [`ProcessKey`] a root.
//!
//! Every row appears exactly once in the forest. Construction is iterative, so a
//! pathological chain cannot overflow the stack while building.

use std::collections::HashMap;

use super::key::{Pid, ProcessKey};
use super::table::{ProcessRow, ProcessTable};

/// One process in the forest: an index into [`ProcessTable::rows`] and its children, in
/// table order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessNode {
    pub index: usize,
    pub children: Vec<ProcessNode>,
}

impl ProcessNode {
    /// This node plus all its descendants.
    pub fn count(&self) -> usize {
        let mut total = 0;
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            total += 1;
            stack.extend(node.children.iter());
        }
        total
    }
}

/// Lookups over one table's rows, by row position.
#[derive(Clone, Debug, Default)]
pub(crate) struct TableIndex {
    /// Key → row (the first row, if a provider repeated a key).
    pub(crate) by_key: HashMap<ProcessKey, usize>,
    /// Pid → the first non-exited row with it.
    pub(crate) live_by_pid: HashMap<Pid, usize>,
    /// Each row's parent row, or `None` for a root.
    pub(crate) parents: Vec<Option<usize>>,
    /// Each row's children, in table order.
    pub(crate) children: Vec<Vec<usize>>,
}

impl TableIndex {
    pub(crate) fn build(rows: &[ProcessRow]) -> Self {
        let mut by_key = HashMap::with_capacity(rows.len());
        let mut live_by_pid = HashMap::with_capacity(rows.len());
        for (i, r) in rows.iter().enumerate() {
            by_key.entry(r.key).or_insert(i);
            if !r.is_exited() {
                live_by_pid.entry(r.key.pid).or_insert(i);
            }
        }
        let parents = parent_indices(rows);
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); rows.len()];
        for (child, parent) in parents.iter().enumerate() {
            if let Some(p) = parent {
                children[*p].push(child);
            }
        }
        Self {
            by_key,
            live_by_pid,
            parents,
            children,
        }
    }
}

impl ProcessTable {
    /// The parent/child forest; roots in table order.
    pub fn tree(&self) -> Vec<ProcessNode> {
        let index = self.index();
        let n = self.len();
        let mut built: Vec<Option<ProcessNode>> = vec![None; n];
        let mut roots = Vec::new();
        for root in (0..n).filter(|&i| index.parents[i].is_none()) {
            // Iterative post-order: a node is built after all its children.
            let mut stack = vec![(root, false)];
            while let Some((i, children_done)) = stack.pop() {
                if children_done {
                    let kids = index.children[i]
                        .iter()
                        .filter_map(|&c| built[c].take())
                        .collect();
                    built[i] = Some(ProcessNode {
                        index: i,
                        children: kids,
                    });
                } else {
                    stack.push((i, true));
                    stack.extend(index.children[i].iter().rev().map(|&c| (c, false)));
                }
            }
            if let Some(node) = built[root].take() {
                roots.push(node);
            }
        }
        roots
    }

    /// The direct children of the process with this key, by the same rules as
    /// [`tree`](Self::tree), in table order.
    pub fn children_of(&self, key: &ProcessKey) -> Vec<&ProcessRow> {
        let Some(i) = self.position(key) else {
            return Vec::new();
        };
        let rows = self.rows();
        self.index().children[i].iter().map(|&c| &rows[c]).collect()
    }

    /// The parent of the process with this key, by the same rules as
    /// [`tree`](Self::tree).
    pub fn parent_of(&self, key: &ProcessKey) -> Option<&ProcessRow> {
        let i = self.position(key)?;
        self.index().parents[i].map(|p| &self.rows()[p])
    }
}

/// For each row, the index of its parent row (or `None` for a root), with recycled
/// pids rejected and cycles broken.
fn parent_indices(rows: &[ProcessRow]) -> Vec<Option<usize>> {
    let mut by_pid: HashMap<Pid, Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        by_pid.entry(r.key.pid).or_default().push(i);
    }
    let mut parents: Vec<Option<usize>> = rows
        .iter()
        .map(|child| {
            let ppid = child.parent.filter(|p| *p != child.key.pid)?;
            by_pid
                .get(&ppid)?
                .iter()
                .copied()
                .filter(|&j| rows[j].key.start_time <= child.key.start_time)
                .max_by_key(|&j| (rows[j].key.start_time, !rows[j].is_exited()))
        })
        .collect();

    // Break cycles: walk each chain; meeting a node already on the current path means a
    // cycle, cut at its smallest key.
    const NEW: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; rows.len()];
    for start in 0..rows.len() {
        let mut path = Vec::new();
        let mut cur = start;
        loop {
            if state[cur] == DONE {
                break;
            }
            if state[cur] == ON_PATH {
                if let Some(pos) = path.iter().position(|&p| p == cur) {
                    let cycle: &[usize] = &path[pos..];
                    if let Some(cut) = cycle.iter().copied().min_by_key(|&j| rows[j].key) {
                        parents[cut] = None;
                    }
                }
                break;
            }
            state[cur] = ON_PATH;
            path.push(cur);
            match parents[cur] {
                Some(p) => cur = p,
                None => break,
            }
        }
        for p in path {
            state[p] = DONE;
        }
    }
    parents
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::table::ProcessStatus;

    fn row(pid: u32, start: u64, parent: Option<u32>) -> ProcessRow {
        ProcessRow {
            parent: parent.map(Pid),
            ..ProcessRow::new(ProcessKey::new(pid, start), format!("p{pid}"))
        }
    }

    /// (pid, children) for readable assertions.
    fn shape(table: &ProcessTable, nodes: &[ProcessNode]) -> Vec<(u32, usize)> {
        nodes
            .iter()
            .map(|n| (table.rows()[n.index].key.pid.0, n.children.len()))
            .collect()
    }

    fn total(nodes: &[ProcessNode]) -> usize {
        nodes.iter().map(ProcessNode::count).sum()
    }

    #[test]
    fn builds_a_simple_forest() {
        let table = ProcessTable::new(vec![
            row(1, 0, None),
            row(100, 10, Some(1)),
            row(101, 11, Some(100)),
            row(102, 12, Some(100)),
            row(2, 0, None),
        ]);
        let forest = table.tree();
        assert_eq!(shape(&table, &forest), vec![(1, 1), (2, 0)]);
        let launchd_child = &forest[0].children[0];
        assert_eq!(
            shape(&table, &launchd_child.children),
            vec![(101, 0), (102, 0)]
        );
        assert_eq!(total(&forest), 5);

        let kids: Vec<u32> = table
            .children_of(&ProcessKey::new(100, 10))
            .iter()
            .map(|r| r.key.pid.0)
            .collect();
        assert_eq!(kids, vec![101, 102]);
        assert_eq!(
            table
                .parent_of(&ProcessKey::new(101, 11))
                .map(|r| r.key.pid),
            Some(Pid(100))
        );
        assert!(table.parent_of(&ProcessKey::new(1, 0)).is_none());
        assert!(table.children_of(&ProcessKey::new(999, 0)).is_empty());
    }

    #[test]
    fn missing_parent_makes_a_root() {
        let table = ProcessTable::new(vec![row(50, 5, Some(49)), row(51, 6, Some(50))]);
        let forest = table.tree();
        assert_eq!(shape(&table, &forest), vec![(50, 1)]);
    }

    #[test]
    fn a_recycled_parent_pid_is_not_adopted() {
        // 300 says its parent is 200, but the current 200 started after it did.
        let table = ProcessTable::new(vec![row(200, 900, None), row(300, 500, Some(200))]);
        let forest = table.tree();
        assert_eq!(shape(&table, &forest), vec![(200, 0), (300, 0)]);
        assert!(table.children_of(&ProcessKey::new(200, 900)).is_empty());
    }

    #[test]
    fn exited_and_live_rows_sharing_a_pid_pick_the_right_parent() {
        let mut old = row(7, 100, None);
        old.status = ProcessStatus::Exited;
        let table = ProcessTable::new(vec![
            old,
            row(7, 400, None),
            row(8, 150, Some(7)), // child of the old 7
            row(9, 450, Some(7)), // child of the new 7
        ]);
        assert_eq!(
            table.parent_of(&ProcessKey::new(8, 150)).map(|r| r.key),
            Some(ProcessKey::new(7, 100))
        );
        assert_eq!(
            table.parent_of(&ProcessKey::new(9, 450)).map(|r| r.key),
            Some(ProcessKey::new(7, 400))
        );
        assert_eq!(total(&table.tree()), 4);
    }

    #[test]
    fn self_parent_is_a_root() {
        let table = ProcessTable::new(vec![row(0, 0, Some(0)), row(1, 0, Some(0))]);
        let forest = table.tree();
        assert_eq!(shape(&table, &forest), vec![(0, 1)]);
    }

    #[test]
    fn cycles_are_broken_at_the_smallest_key() {
        // Equal start times so the start-time check does not already break it.
        let table = ProcessTable::new(vec![
            row(12, 0, Some(11)),
            row(10, 0, Some(12)),
            row(11, 0, Some(10)),
            row(13, 0, Some(12)),
        ]);
        let forest = table.tree();
        assert_eq!(total(&forest), 4);
        assert_eq!(forest.len(), 1);
        assert_eq!(table.rows()[forest[0].index].key.pid, Pid(10));
        assert!(table.parent_of(&ProcessKey::new(10, 0)).is_none());
    }

    #[test]
    fn deep_chains_build_without_recursion() {
        let depth = 2_000u32;
        let rows = (0..depth)
            .map(|i| row(i + 1, u64::from(i), if i == 0 { None } else { Some(i) }))
            .collect();
        let table = ProcessTable::new(rows);
        let forest = table.tree();
        assert_eq!(forest.len(), 1);
        assert_eq!(total(&forest), depth as usize);
    }

    #[test]
    fn empty_table_has_an_empty_forest() {
        assert!(ProcessTable::default().tree().is_empty());
    }
}
