//! Undo and redo: each step is a list of replacements plus the selection around them.

use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Edit {
    /// Where the replacement went, in the text as it was before it.
    pub range: Range<usize>,
    pub old: String,
    pub new: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Typing and IME composition; runs of it undo together.
    Typing,
    /// Backspace and Delete; runs of it undo together.
    Deleting,
    /// Everything else is its own step.
    Other,
}

#[derive(Clone, Debug)]
pub struct Selection {
    pub range: Range<usize>,
    pub reversed: bool,
}

#[derive(Clone, Debug)]
pub struct Transaction {
    pub edits: Vec<Edit>,
    pub before: Selection,
    pub after: Selection,
    kind: EditKind,
    time: Instant,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// The next edit starts a new step even if it could join the last one.
    sealed: bool,
}

const GROUP_INTERVAL: Duration = Duration::from_millis(1000);
const MAX_STEPS: usize = 10_000;

impl History {
    /// Records a finished change; consecutive typing (or deleting) joins the last step.
    pub fn push(&mut self, edits: Vec<Edit>, before: Selection, after: Selection, kind: EditKind) {
        if edits.is_empty() {
            return;
        }
        self.redo.clear();
        let now = Instant::now();
        if !self.sealed && kind != EditKind::Other {
            if let Some(last) = self.undo.last_mut() {
                let joins = last.kind == kind
                    && now.duration_since(last.time) < GROUP_INTERVAL
                    && last.after.range == before.range
                    // A new line starts a new step, as in other editors.
                    && !edits.iter().any(|e| e.new.contains('\n'));
                if joins {
                    last.edits.extend(edits);
                    last.after = after;
                    last.time = now;
                    return;
                }
            }
        }
        self.sealed = false;
        self.undo.push(Transaction { edits, before, after, kind, time: now });
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
    }

    /// Stops the next edit from joining the current step (after the cursor moves).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn pop_undo(&mut self) -> Option<Transaction> {
        let transaction = self.undo.pop()?;
        self.sealed = true;
        Some(transaction)
    }

    pub fn push_redo(&mut self, transaction: Transaction) {
        self.redo.push(transaction);
    }

    pub fn pop_redo(&mut self) -> Option<Transaction> {
        self.redo.pop()
    }

    pub fn push_undo_from_redo(&mut self, transaction: Transaction) {
        self.undo.push(transaction);
        self.sealed = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
