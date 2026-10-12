use std::sync::Arc;

const MAX_ENTRIES: usize = 100;
const MAX_BYTES: usize = 32 * 1024 * 1024;

struct Change {
    before: Arc<str>,
    after: Arc<str>,
    group: Option<u64>,
}

#[derive(Default)]
pub(crate) struct CanvasHistory {
    changes: Vec<Change>,
    position: usize,
}

impl CanvasHistory {
    pub(crate) fn clear(&mut self) {
        self.changes.clear();
        self.position = 0;
    }

    pub(crate) fn record(&mut self, before: Arc<str>, after: Arc<str>, group: Option<u64>) {
        if before == after {
            return;
        }
        if !self.matches(&before) {
            self.clear();
        }
        let branching = self.position < self.changes.len();
        self.changes.truncate(self.position);
        if !branching
            && group.is_some()
            && self
                .changes
                .last()
                .is_some_and(|change| change.group == group)
        {
            self.changes.last_mut().unwrap().after = after;
            if self
                .changes
                .last()
                .is_some_and(|change| change.before == change.after)
            {
                self.changes.pop();
            }
        } else {
            self.changes.push(Change {
                before,
                after,
                group,
            });
        }
        self.position = self.changes.len();
        let mut bytes = self
            .changes
            .iter()
            .map(|change| change.before.len().saturating_add(change.after.len()))
            .fold(0usize, usize::saturating_add);
        let mut remove = 0;
        while remove < self.changes.len()
            && (self.changes.len() - remove > MAX_ENTRIES || bytes > MAX_BYTES)
        {
            let change = &self.changes[remove];
            bytes = bytes.saturating_sub(change.before.len().saturating_add(change.after.len()));
            remove += 1;
        }
        self.changes.drain(..remove);
        self.position -= remove;
    }

    fn matches(&self, current: &str) -> bool {
        if self.position > 0 {
            self.changes[self.position - 1].after.as_ref() == current
        } else {
            self.changes
                .first()
                .is_none_or(|change| change.before.as_ref() == current)
        }
    }

    pub(crate) fn undo(&mut self, current: &str) -> Option<Arc<str>> {
        if !self.matches(current) {
            self.clear();
            return None;
        }
        let next = self.position.checked_sub(1)?;
        self.position = next;
        Some(self.changes[next].before.clone())
    }

    pub(crate) fn redo(&mut self, current: &str) -> Option<Arc<str>> {
        if !self.matches(current) {
            self.clear();
            return None;
        }
        let change = self.changes.get(self.position)?;
        let after = change.after.clone();
        self.position += 1;
        Some(after)
    }

    pub(crate) fn has_undo(&self) -> bool {
        self.position > 0
    }

    pub(crate) fn has_redo(&self) -> bool {
        self.position < self.changes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_history_preserves_exact_bytes_and_branches() {
        let mut history = CanvasHistory::default();
        history.record("{\n \"future\":42\n}".into(), "second".into(), None);
        history.record("second".into(), "third".into(), None);
        assert_eq!(history.undo("third").as_deref(), Some("second"));
        assert_eq!(
            history.undo("second").as_deref(),
            Some("{\n \"future\":42\n}")
        );
        assert_eq!(
            history.redo("{\n \"future\":42\n}").as_deref(),
            Some("second")
        );
        history.record("second".into(), "branch".into(), None);
        assert!(!history.has_redo());
        assert_eq!(history.undo("branch").as_deref(), Some("second"));
    }

    #[test]
    fn canvas_history_coalesces_only_the_same_edit_session() {
        let mut history = CanvasHistory::default();
        history.record("zero".into(), "a".into(), Some(1));
        history.record("a".into(), "ab".into(), Some(1));
        history.record("ab".into(), "abc".into(), Some(2));
        assert_eq!(history.undo("abc").as_deref(), Some("ab"));
        assert_eq!(history.undo("ab").as_deref(), Some("zero"));
        assert!(!history.has_undo());
        assert_eq!(history.redo("zero").as_deref(), Some("ab"));
        history.record("ab".into(), "new".into(), Some(1));
        assert_eq!(history.undo("new").as_deref(), Some("ab"));
        history.clear();
        history.record("start".into(), "typing".into(), Some(3));
        history.record("typing".into(), "start".into(), Some(3));
        assert!(!history.has_undo());
    }

    #[test]
    fn canvas_history_never_overwrites_external_changes() {
        let mut history = CanvasHistory::default();
        history.record("original".into(), "edited".into(), None);
        assert!(history.undo("external").is_none());
        assert!(!history.has_undo());
        assert!(!history.has_redo());
        history.record("external".into(), "new".into(), None);
        assert_eq!(history.undo("new").as_deref(), Some("external"));
        assert!(history.redo("second external").is_none());
        history.record("second external".into(), "last".into(), Some(9));
        history.record("third external".into(), "final".into(), Some(9));
        assert_eq!(history.undo("final").as_deref(), Some("third external"));
        assert!(!history.has_undo());
    }

    #[test]
    fn canvas_history_noop_preserves_redo_and_bounds_retention() {
        let mut history = CanvasHistory::default();
        history.record("a".into(), "b".into(), None);
        assert_eq!(history.undo("b").as_deref(), Some("a"));
        history.record("a".into(), "a".into(), None);
        assert_eq!(history.redo("a").as_deref(), Some("b"));
        for index in 0..MAX_ENTRIES + 20 {
            history.record(
                index.to_string().into(),
                (index + 1).to_string().into(),
                None,
            );
        }
        assert_eq!(history.changes.len(), MAX_ENTRIES);
        let large: Arc<str> = "x".repeat(MAX_BYTES + 1).into();
        history.record((MAX_ENTRIES + 20).to_string().into(), large, None);
        assert!(!history.has_undo());
    }
}
