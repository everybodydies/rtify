//! Playlist changes that Ctrl+Z (or the toast's "undo") reverts, newest first. An undo that
//! Spotify refuses goes back on the stack, so trying again works.

/// At most this many changes are remembered.
pub const LIMIT: usize = 30;

/// A playlist change. `P` is the playlist as the UI knows it.
#[derive(Debug, Clone, PartialEq)]
pub enum UndoOp<P> {
    Added {
        playlist: P,
        uris: Vec<String>,
    },
    /// `(uri, position)` in the playlist's order before removal.
    Removed {
        playlist: P,
        entries: Vec<(String, u32)>,
    },
}

#[derive(Debug, Clone)]
pub struct UndoStack<P> {
    ops: Vec<UndoOp<P>>,
}

impl<P> Default for UndoStack<P> {
    fn default() -> Self {
        Self { ops: Vec::new() }
    }
}

impl<P> UndoStack<P> {
    pub fn push(&mut self, op: UndoOp<P>) {
        if matches!(&op, UndoOp::Added { uris, .. } if uris.is_empty())
            || matches!(&op, UndoOp::Removed { entries, .. } if entries.is_empty())
        {
            return;
        }
        self.ops.push(op);
        if self.ops.len() > LIMIT {
            self.ops.remove(0);
        }
    }

    pub fn pop(&mut self) -> Option<UndoOp<P>> {
        self.ops.pop()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// The inserts that put removed songs back where they were. Ascending positions: each insert
/// lands where the song used to be, because everything before it is already back.
pub fn restore_steps(mut entries: Vec<(String, u32)>) -> Vec<(String, u32)> {
    entries.sort_by_key(|(_, pos)| *pos);
    entries.dedup();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spotify's semantics: removing a uri removes every occurrence.
    fn remove(list: &mut Vec<String>, uris: &[String]) {
        list.retain(|u| !uris.contains(u));
    }

    fn insert(list: &mut Vec<String>, uri: &str, pos: u32) {
        let pos = (pos as usize).min(list.len());
        list.insert(pos, uri.to_owned());
    }

    /// What the collection view records before a removal (`positions_of`).
    fn positions(list: &[String], uris: &[String]) -> Vec<(String, u32)> {
        list.iter()
            .enumerate()
            .filter(|(_, u)| uris.contains(u))
            .map(|(i, u)| (u.clone(), i as u32))
            .collect()
    }

    fn songs(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("spotify:track:{i}")).collect()
    }

    #[test]
    fn removed_songs_come_back_in_their_places() {
        let original = songs(8);
        let gone = vec![original[6].clone(), original[0].clone(), original[3].clone()];
        let mut list = original.clone();
        let entries = positions(&list, &gone);
        remove(&mut list, &gone);
        // Recorded in any order, restored in ascending order.
        let mut shuffled = entries.clone();
        shuffled.reverse();
        for (uri, pos) in restore_steps(shuffled) {
            insert(&mut list, &uri, pos);
        }
        assert_eq!(list, original);
    }

    #[test]
    fn duplicates_and_neighbours_survive_a_round_trip() {
        // xorshift: deterministic without a dependency.
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..500 {
            let pool = songs(12);
            let len = 1 + (next() % 40) as usize;
            let original: Vec<String> = (0..len).map(|_| pool[(next() % 12) as usize].clone()).collect();
            let picks = 1 + (next() % 4) as usize;
            let gone: Vec<String> = (0..picks).map(|_| original[(next() % len as u64) as usize].clone()).collect();
            let mut list = original.clone();
            let entries = positions(&list, &gone);
            remove(&mut list, &gone);
            for (uri, pos) in restore_steps(entries) {
                insert(&mut list, &uri, pos);
            }
            assert_eq!(list, original, "removed {gone:?}");
        }
    }

    #[test]
    fn a_partly_failed_restore_can_be_finished_later() {
        let original = songs(10);
        let gone = vec![original[1].clone(), original[4].clone(), original[8].clone()];
        let mut list = original.clone();
        let steps = restore_steps(positions(&list, &gone));
        remove(&mut list, &gone);
        // The first insert succeeds, then Spotify fails; the rest goes back on the stack.
        insert(&mut list, &steps[0].0, steps[0].1);
        let mut stack = UndoStack::default();
        stack.push(UndoOp::Removed {
            playlist: "p",
            entries: steps[1..].to_vec(),
        });
        let Some(UndoOp::Removed { entries, .. }) = stack.pop() else {
            panic!("the rest of the restore is gone");
        };
        for (uri, pos) in restore_steps(entries) {
            insert(&mut list, &uri, pos);
        }
        assert_eq!(list, original);
    }

    #[test]
    fn newest_change_is_undone_first() {
        let mut stack = UndoStack::default();
        stack.push(UndoOp::Added {
            playlist: "a",
            uris: vec!["x".into()],
        });
        stack.push(UndoOp::Removed {
            playlist: "b",
            entries: vec![("y".into(), 2)],
        });
        assert!(matches!(stack.pop(), Some(UndoOp::Removed { playlist: "b", .. })));
        assert!(matches!(stack.pop(), Some(UndoOp::Added { playlist: "a", .. })));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn empty_changes_are_not_recorded() {
        let mut stack: UndoStack<&str> = UndoStack::default();
        stack.push(UndoOp::Added {
            playlist: "a",
            uris: vec![],
        });
        stack.push(UndoOp::Removed {
            playlist: "a",
            entries: vec![],
        });
        assert!(stack.is_empty());
    }

    #[test]
    fn the_stack_is_bounded() {
        let mut stack = UndoStack::default();
        for i in 0..LIMIT + 5 {
            stack.push(UndoOp::Added {
                playlist: i,
                uris: vec!["x".into()],
            });
        }
        assert_eq!(stack.len(), LIMIT);
        let mut last = None;
        while let Some(UndoOp::Added { playlist, .. }) = stack.pop() {
            last = Some(playlist);
        }
        assert_eq!(last, Some(5));
    }
}
