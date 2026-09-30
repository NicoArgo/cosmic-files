// SPDX-License-Identifier: GPL-3.0-only

//! POP Flow: free placement of desktop icons on a grid.
//!
//! Every icon owns a cell (row, col) that the user chose by dragging it; the
//! grid only gives the drop a gentle snap. Icons nobody placed yet (new files,
//! or everything before the first drag) take the first free cell in the
//! classic order: down a column, then the next column. Rows are unbounded
//! below the screen — that's what makes the desktop scroll.
//!
//! Pure: no widgets, no filesystem except `load`/`save` at the bottom.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

/// (row, col)
pub type Cell = (usize, usize);

/// Cells for `names` (in display order). A saved cell is kept when it is still
/// on the grid (`col < cols`) and nobody before claimed it; everything else is
/// auto-placed with [`first_free`].
pub fn assign(
    names: &[&str],
    saved: &HashMap<String, Cell>,
    cols: usize,
    rows_fit: usize,
    columns_limit: usize,
) -> Vec<Cell> {
    let cols = cols.max(1);
    let mut taken = HashSet::new();
    let mut out: Vec<Option<Cell>> = names
        .iter()
        .map(|name| {
            let cell = saved.get(*name).copied()?;
            (cell.1 < cols && taken.insert(cell)).then_some(cell)
        })
        .collect();
    let limit = if columns_limit == 0 { cols } else { columns_limit.min(cols) };
    for slot in &mut out {
        if slot.is_none() {
            let cell = first_free(&taken, limit, rows_fit);
            taken.insert(cell);
            *slot = Some(cell);
        }
    }
    out.into_iter().flatten().collect()
}

/// The first free cell going down each of the first `cols` columns, one
/// screenful (`rows_fit` rows) at a time: the screen fills column by column
/// exactly like upstream, and only when it is full do icons go below it.
pub fn first_free(taken: &HashSet<Cell>, cols: usize, rows_fit: usize) -> Cell {
    let (cols, rows_fit) = (cols.max(1), rows_fit.max(1));
    for band in 0.. {
        for col in 0..cols {
            for row in band * rows_fit..(band + 1) * rows_fit {
                if !taken.contains(&(row, col)) {
                    return (row, col);
                }
            }
        }
    }
    unreachable!("the grid is unbounded below")
}

/// The free cell closest to `target` (breadth-first over the 4-neighborhood,
/// so ties go to the nearest in steps), within `cols` columns and rows ≥ 0.
pub fn nearest_free(taken: &HashSet<Cell>, target: Cell, cols: usize) -> Cell {
    let cols = cols.max(1);
    let target = (target.0, target.1.min(cols - 1));
    let mut seen = HashSet::from([target]);
    let mut queue = VecDeque::from([target]);
    while let Some(cell @ (row, col)) = queue.pop_front() {
        if !taken.contains(&cell) {
            return cell;
        }
        let neighbors = [
            (row.checked_sub(1), Some(col)),
            (Some(row + 1), Some(col)),
            (Some(row), col.checked_sub(1)),
            (Some(row), (col + 1 < cols).then_some(col + 1)),
        ];
        for (r, c) in neighbors {
            if let (Some(r), Some(c)) = (r, c)
                && seen.insert((r, c))
            {
                queue.push_back((r, c));
            }
        }
    }
    unreachable!("the grid is unbounded below")
}

/// Moves `moving` (names with their current cells) by the offset that takes
/// `from` to `to`, keeping the group's shape. A cell already held by an icon
/// that isn't moving sends the arriving icon to the nearest free cell instead,
/// so nothing is ever stacked. Returns the new cells, in the same order.
pub fn move_group(
    all: &HashMap<String, Cell>,
    moving: &[(String, Cell)],
    from: Cell,
    to: Cell,
    cols: usize,
) -> Vec<(String, Cell)> {
    let cols = cols.max(1);
    let moving_names: HashSet<&str> = moving.iter().map(|(n, _)| n.as_str()).collect();
    let mut taken: HashSet<Cell> = all
        .iter()
        .filter(|(name, _)| !moving_names.contains(name.as_str()))
        .map(|(_, cell)| *cell)
        .collect();
    let (dr, dc) = (to.0 as isize - from.0 as isize, to.1 as isize - from.1 as isize);
    moving
        .iter()
        .map(|(name, (row, col))| {
            let wanted = (
                (*row as isize + dr).max(0) as usize,
                (*col as isize + dc).clamp(0, cols as isize - 1) as usize,
            );
            let cell = nearest_free(&taken, wanted, cols);
            taken.insert(cell);
            (name.clone(), cell)
        })
        .collect()
}

/// Grid geometry of the desktop as last drawn, in content coordinates (the
/// scroll offset already added).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub padding: f32,
    pub item_width: f32,
    pub item_height: f32,
    pub column_step: f32,
    pub row_step: f32,
    pub cols: usize,
}

impl Geometry {
    /// The cell whose center is nearest to a point — the "snap".
    pub fn cell_at(&self, x: f32, y: f32) -> Cell {
        let snap = |v: f32, size: f32, step: f32| {
            ((v - self.padding - size / 2.0) / step.max(1.0)).round().max(0.0) as usize
        };
        let col = snap(x, self.item_width, self.column_step).min(self.cols.max(1) - 1);
        (snap(y, self.item_height, self.row_step), col)
    }
}

/// Where the positions live: per output (monitor), then per file name.
fn store_path() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("cosmic-files").join("desktop-positions.ron"))
}

type Store = BTreeMap<String, BTreeMap<String, (usize, usize)>>;

fn read_store() -> Store {
    store_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| ron::from_str(&s).ok())
        .unwrap_or_default()
}

/// Saved cells for one output.
pub fn load(output: &str) -> HashMap<String, Cell> {
    read_store()
        .remove(output)
        .map(|m| m.into_iter().collect())
        .unwrap_or_default()
}

/// Replaces one output's cells, leaving other outputs alone. Written to a
/// temporary file and renamed, so a crash can't leave half a layout.
pub fn save(output: &str, cells: &HashMap<String, Cell>) -> std::io::Result<()> {
    let Some(path) = store_path() else {
        return Ok(());
    };
    let mut store = read_store();
    store.insert(output.to_owned(), cells.iter().map(|(k, v)| (k.clone(), *v)).collect());
    let text = ron::ser::to_string_pretty(&store, ron::ser::PrettyConfig::default())
        .map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("ron~");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, Cell)]) -> HashMap<String, Cell> {
        entries.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    #[test]
    fn nothing_saved_fills_like_upstream() {
        // 3 rows fit, 2 columns allowed: down col 0, then col 1, then below.
        let names = ["a", "b", "c", "d", "e", "f", "g"];
        let cells = assign(&names, &HashMap::new(), 10, 3, 2);
        assert_eq!(cells, vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1), (3, 0)]);
    }

    #[test]
    fn saved_cells_are_kept_and_new_icons_take_free_ones() {
        let saved = map(&[("a", (5, 7)), ("b", (0, 0))]);
        let cells = assign(&["a", "b", "new"], &saved, 10, 3, 4);
        assert_eq!(cells, vec![(5, 7), (0, 0), (1, 0)]);
    }

    #[test]
    fn saved_cell_off_a_narrower_screen_is_replaced() {
        let saved = map(&[("a", (0, 12))]);
        assert_eq!(assign(&["a"], &saved, 8, 3, 4), vec![(0, 0)]);
    }

    #[test]
    fn two_icons_never_share_a_cell() {
        let saved = map(&[("a", (1, 1)), ("b", (1, 1))]);
        let cells = assign(&["a", "b"], &saved, 5, 3, 4);
        assert_ne!(cells[0], cells[1]);
    }

    #[test]
    fn nearest_free_prefers_the_target_then_its_neighbors() {
        let taken = HashSet::from([(2, 2)]);
        assert_eq!(nearest_free(&taken, (4, 4), 10), (4, 4));
        let found = nearest_free(&taken, (2, 2), 10);
        let dist = found.0.abs_diff(2) + found.1.abs_diff(2);
        assert_eq!(dist, 1);
    }

    #[test]
    fn nearest_free_stays_on_the_grid() {
        let taken: HashSet<Cell> = (0..3).map(|c| (0, c)).collect();
        let (row, col) = nearest_free(&taken, (0, 9), 3);
        assert!(col < 3);
        assert_eq!(row, 1);
    }

    #[test]
    fn group_keeps_its_shape() {
        let all = map(&[("a", (0, 0)), ("b", (1, 0)), ("x", (0, 5))]);
        let moving = vec![("a".to_string(), (0, 0)), ("b".to_string(), (1, 0))];
        let moved = move_group(&all, &moving, (0, 0), (3, 2), 10);
        assert_eq!(moved, vec![("a".into(), (3, 2)), ("b".into(), (4, 2))]);
    }

    #[test]
    fn dropping_on_an_icon_lands_next_to_it() {
        let all = map(&[("a", (0, 0)), ("x", (2, 2))]);
        let moved = move_group(&all, &[("a".to_string(), (0, 0))], (0, 0), (2, 2), 10);
        let cell = moved[0].1;
        assert_ne!(cell, (2, 2));
        assert_eq!(cell.0.abs_diff(2) + cell.1.abs_diff(2), 1);
    }

    #[test]
    fn a_group_can_move_onto_its_own_old_cells() {
        // Shift a column of 3 down by one: each lands where its neighbor was.
        let all = map(&[("a", (0, 0)), ("b", (1, 0)), ("c", (2, 0))]);
        let moving: Vec<_> = ["a", "b", "c"]
            .iter()
            .zip([(0, 0), (1, 0), (2, 0)])
            .map(|(n, c)| (n.to_string(), c))
            .collect();
        let moved = move_group(&all, &moving, (0, 0), (1, 0), 4);
        assert_eq!(
            moved.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
            vec![(1, 0), (2, 0), (3, 0)]
        );
    }

    #[test]
    fn moving_below_the_screen_is_allowed() {
        let all = map(&[("a", (0, 0))]);
        let moved = move_group(&all, &[("a".to_string(), (0, 0))], (0, 0), (40, 1), 4);
        assert_eq!(moved[0].1, (40, 1));
    }

    #[test]
    fn snap_picks_the_nearest_cell_center() {
        let g = Geometry {
            padding: 8.0,
            item_width: 100.0,
            item_height: 120.0,
            column_step: 110.0,
            row_step: 130.0,
            cols: 5,
        };
        // Center of (0,0) is (58, 68); of (1,1) is (168, 198).
        assert_eq!(g.cell_at(58.0, 68.0), (0, 0));
        assert_eq!(g.cell_at(160.0, 190.0), (1, 1));
        // Slightly past the midpoint between two cells snaps to the nearer one.
        assert_eq!(g.cell_at(120.0, 68.0), (0, 1));
        // Far right clamps to the last column; far up clamps to row 0.
        assert_eq!(g.cell_at(5000.0, -300.0), (0, 4));
        // Far down is fine: rows are unbounded.
        assert_eq!(g.cell_at(58.0, 68.0 + 130.0 * 20.0), (20, 0));
    }
}
