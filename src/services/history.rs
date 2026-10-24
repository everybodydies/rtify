//! Back/forward history of the main view, as in a browser: going somewhere new drops the
//! forward entries, revisiting the current entry adds nothing.

/// At most this many entries; the oldest go first.
pub const LIMIT: usize = 50;

#[derive(Debug, Clone)]
pub struct History<T> {
    entries: Vec<T>,
    pos: usize,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            pos: 0,
        }
    }
}

impl<T: Clone + PartialEq> History<T> {
    /// Record a visit. Returns false when it is the current entry already.
    pub fn push(&mut self, entry: T) -> bool {
        if self.current() == Some(&entry) {
            return false;
        }
        self.entries.truncate(self.pos + 1);
        self.entries.push(entry);
        if self.entries.len() > LIMIT {
            self.entries.remove(0);
        }
        self.pos = self.entries.len() - 1;
        true
    }

    /// Step back (negative) or forward (positive) and return the entry to show, or `None` at
    /// either end (the position stays put then).
    pub fn go(&mut self, delta: i32) -> Option<T> {
        let pos = self.pos as i64 + delta as i64;
        if self.entries.is_empty() || pos < 0 || pos >= self.entries.len() as i64 {
            return None;
        }
        self.pos = pos as usize;
        Some(self.entries[self.pos].clone())
    }

    /// Update the current entry in place (a search refined while typing), or record it when
    /// the history is empty. Forward entries stay.
    pub fn replace_current(&mut self, entry: T) {
        match self.entries.get_mut(self.pos) {
            Some(e) => *e = entry,
            None => {
                self.push(entry);
            }
        }
    }

    pub fn current(&self) -> Option<&T> {
        self.entries.get(self.pos)
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visited(pages: &[&'static str]) -> History<&'static str> {
        let mut h = History::default();
        for p in pages {
            h.push(*p);
        }
        h
    }

    #[test]
    fn empty_history_goes_nowhere() {
        let mut h: History<&str> = History::default();
        assert_eq!(h.go(-1), None);
        assert_eq!(h.go(1), None);
        assert!(!h.can_back() && !h.can_forward());
    }

    #[test]
    fn back_and_forward_walk_the_visits() {
        let mut h = visited(&["a", "b", "c"]);
        assert!(h.can_back() && !h.can_forward());
        assert_eq!(h.go(-1), Some("b"));
        assert_eq!(h.go(-1), Some("a"));
        assert_eq!(h.go(-1), None);
        assert_eq!(h.current(), Some(&"a"));
        assert!(!h.can_back() && h.can_forward());
        assert_eq!(h.go(1), Some("b"));
        assert_eq!(h.go(1), Some("c"));
        assert_eq!(h.go(1), None);
        assert_eq!(h.current(), Some(&"c"));
    }

    #[test]
    fn a_new_visit_drops_the_forward_entries() {
        let mut h = visited(&["a", "b", "c"]);
        h.go(-2);
        assert!(h.push("d"));
        assert!(!h.can_forward());
        assert_eq!(h.go(-1), Some("a"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn clicking_the_open_page_again_adds_nothing() {
        let mut h = visited(&["a", "b"]);
        assert!(!h.push("b"));
        assert_eq!(h.len(), 2);
        assert_eq!(h.go(-1), Some("a"));
        // Going back and then opening the page you are on keeps the forward entries.
        assert!(!h.push("a"));
        assert!(h.can_forward());
        // A page visited earlier, but not the current one, is a new visit.
        assert!(h.push("b"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn the_oldest_entries_fall_off() {
        let pages: Vec<String> = (0..LIMIT + 10).map(|i| format!("p{i}")).collect();
        let mut h = History::default();
        for p in &pages {
            h.push(p.clone());
        }
        assert_eq!(h.len(), LIMIT);
        let mut oldest = None;
        while let Some(p) = h.go(-1) {
            oldest = Some(p);
        }
        assert_eq!(oldest.as_deref(), Some("p10"));
        // Forward again reaches the newest.
        let mut newest = None;
        while let Some(p) = h.go(1) {
            newest = Some(p);
        }
        assert_eq!(newest.as_deref(), Some(pages.last().unwrap().as_str()));
    }

    #[test]
    fn refining_the_current_entry_replaces_it() {
        let mut h = visited(&["a", "b"]);
        h.replace_current("b2");
        assert_eq!(h.len(), 2);
        assert_eq!(h.go(-1), Some("a"));
        assert_eq!(h.go(1), Some("b2"));
        let mut empty = History::default();
        empty.replace_current("x");
        assert_eq!(empty.current(), Some(&"x"));
    }

    #[test]
    fn a_big_jump_past_the_end_is_ignored() {
        let mut h = visited(&["a", "b"]);
        assert_eq!(h.go(5), None);
        assert_eq!(h.go(-5), None);
        assert_eq!(h.current(), Some(&"b"));
    }
}
