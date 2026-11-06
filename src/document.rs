//! The open paradigm plus undo/redo and selection, shared by all panels.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::model::{Block, Paradigm};

const UNDO_LIMIT: usize = 200;
/// Successive edits of the same field (e.g. dragging a number) within this time are one undo step.
const MERGE_SECS: f64 = 1.2;

pub struct Document {
    pub paradigm: Paradigm,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub selection: Vec<String>,
    undo: Vec<Paradigm>,
    redo: Vec<Paradigm>,
    pending: Option<Paradigm>,
    last_merge: Option<(String, Instant)>,
    /// bumped on every change; views use it to refresh caches
    pub version: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new(Paradigm::default())
    }
}

impl Document {
    pub fn new(p: Paradigm) -> Self {
        Document {
            paradigm: p,
            path: None,
            dirty: false,
            selection: vec![],
            undo: vec![],
            redo: vec![],
            pending: None,
            last_merge: None,
            version: 0,
        }
    }

    // ---- selection -----------------------------------------------------------
    pub fn select(&mut self, ids: Vec<String>) {
        let ids: Vec<String> = ids.into_iter().filter(|i| self.paradigm.block(i).is_some()).collect();
        if ids != self.selection {
            self.selection = ids;
        }
    }

    pub fn selected_blocks(&self) -> Vec<&Block> {
        self.selection.iter().filter_map(|i| self.paradigm.block(i)).collect()
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selection.iter().any(|s| s == id)
    }

    // ---- undo ----------------------------------------------------------------
    /// Remember the state before a (possibly multi-step) interactive edit, e.g. a drag.
    pub fn begin_edit(&mut self) {
        if self.pending.is_none() {
            self.pending = Some(self.paradigm.clone());
        }
    }

    pub fn commit_edit(&mut self) {
        if let Some(before) = self.pending.take() {
            if before != self.paradigm {
                self.push(before);
            }
        }
        self.version += 1;
    }

    pub fn cancel_edit(&mut self) {
        if let Some(before) = self.pending.take() {
            self.restore(before);
        }
    }

    pub fn in_edit(&self) -> bool {
        self.pending.is_some()
    }

    /// Run `f` on the paradigm as one undoable step.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Paradigm) -> R) -> R {
        self.edit_merge(None, f)
    }

    /// Like [`Self::edit`]; consecutive edits with the same `merge_key` form one undo step.
    pub fn edit_merge<R>(&mut self, merge_key: Option<&str>, f: impl FnOnce(&mut Paradigm) -> R) -> R {
        let before = self.paradigm.clone();
        let r = f(&mut self.paradigm);
        if before != self.paradigm {
            let merge = match (merge_key, &self.last_merge) {
                (Some(k), Some((lk, t))) => k == lk && t.elapsed().as_secs_f64() < MERGE_SECS && !self.undo.is_empty(),
                _ => false,
            };
            if !merge {
                self.push(before);
            } else {
                self.redo.clear();
                self.dirty = true;
            }
            self.last_merge = merge_key.map(|k| (k.to_string(), Instant::now()));
            self.version += 1;
        }
        r
    }

    fn push(&mut self, state: Paradigm) {
        self.undo.push(state);
        if self.undo.len() > UNDO_LIMIT {
            let n = self.undo.len() - UNDO_LIMIT;
            self.undo.drain(..n);
        }
        self.redo.clear();
        self.dirty = true;
        self.last_merge = None;
    }

    fn restore(&mut self, mut state: Paradigm) {
        state.hardware = self.paradigm.hardware.clone(); // hardware setup is not part of the undo history
        self.paradigm = state;
        let sel: Vec<String> = self.selection.clone();
        self.selection = sel.into_iter().filter(|i| self.paradigm.block(i).is_some()).collect();
        self.version += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.paradigm.clone());
            self.restore(s);
            self.dirty = true;
            self.last_merge = None;
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.paradigm.clone());
            self.restore(s);
            self.dirty = true;
            self.last_merge = None;
        }
    }

    /// A change that is not undoable (e.g. the hardware setup) but makes the file dirty.
    pub fn touch(&mut self) {
        self.dirty = true;
        self.version += 1;
    }

    // ---- files ---------------------------------------------------------------
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        let p = Paradigm::load(path)?;
        self.reset(p);
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.paradigm.save(path)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.version += 1;
        Ok(())
    }

    pub fn reset(&mut self, p: Paradigm) {
        self.paradigm = p;
        self.path = None;
        self.dirty = false;
        self.selection.clear();
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.last_merge = None;
        self.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_merge() {
        let mut d = Document::default();
        d.edit(|p| p.blocks.push(Block::create("fixation", 0.0, None)));
        assert!(d.can_undo() && d.dirty);
        let id = d.paradigm.blocks[0].id.clone();
        for k in 1..5 {
            d.edit_merge(Some("dur"), |p| p.block_mut(&id).unwrap().duration = 10.0 + k as f64);
        }
        assert_eq!(d.paradigm.blocks[0].duration, 14.0);
        d.undo(); // one step back over the whole drag
        assert_eq!(d.paradigm.blocks[0].duration, 10.0);
        d.undo();
        assert!(d.paradigm.blocks.is_empty());
        d.redo();
        d.redo();
        assert_eq!(d.paradigm.blocks[0].duration, 14.0);
        d.paradigm.hardware.trigger_key = "5".into();
        d.undo();
        assert_eq!(d.paradigm.hardware.trigger_key, "5"); // hardware is not undone
    }
}
