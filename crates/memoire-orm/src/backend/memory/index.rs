//! Hash indexes that store row positions only. A lookup hashes the probed
//! value and compares it with the value in the row, so keys are never copied:
//! inserting a row with a unique `email` costs no allocation for its index.

use std::hash::BuildHasher;

use foldhash::fast::RandomState;
use hashbrown::HashTable;

use crate::value::Value;

use super::eval::{KeyRef, key_eq};

#[derive(Clone)]
pub(crate) struct Index {
    pub(crate) col: usize,
    /// Primary key or `unique`: at most one row per value.
    pub(crate) unique: bool,
    hasher: RandomState,
    /// `(hash of the value, row)`; NULLs are not indexed.
    entries: HashTable<(u64, u32)>,
}

impl Index {
    pub(crate) fn new(col: usize, unique: bool) -> Self {
        Index { col, unique, hasher: RandomState::default(), entries: HashTable::new() }
    }

    /// The hash this index files `v` under.
    #[inline]
    pub(crate) fn hash(&self, v: &Value) -> u64 {
        self.hasher.hash_one(KeyRef(v))
    }

    /// A row whose value equals `v` (the only one for a unique index).
    #[inline]
    pub(crate) fn find(&self, rows: &[Vec<Value>], v: &Value) -> Option<usize> {
        if v.is_null() {
            return None;
        }
        self.find_hashed(rows, v, self.hash(v))
    }

    /// [`find`](Self::find) with `h == self.hash(v)` already computed.
    #[inline]
    pub(crate) fn find_hashed(&self, rows: &[Vec<Value>], v: &Value, h: u64) -> Option<usize> {
        self.entries.find(h, |&(eh, r)| eh == h && key_eq(&rows[r as usize][self.col], v)).map(|&(_, r)| r as usize)
    }

    /// Every row whose value equals `v`, in table order.
    pub(crate) fn find_all(&self, rows: &[Vec<Value>], v: &Value) -> Vec<usize> {
        if v.is_null() {
            return Vec::new();
        }
        if self.unique {
            return self.find(rows, v).into_iter().collect();
        }
        let h = self.hash(v);
        let mut out: Vec<usize> = self
            .entries
            .iter_hash(h)
            .filter(|&&(eh, r)| eh == h && key_eq(&rows[r as usize][self.col], v))
            .map(|&(_, r)| r as usize)
            .collect();
        out.sort_unstable();
        out
    }

    /// Indexes row `row` (already stored in `rows`).
    #[inline]
    pub(crate) fn insert(&mut self, rows: &[Vec<Value>], row: usize) {
        let v = &rows[row][self.col];
        if !v.is_null() {
            let h = self.hash(v);
            self.insert_hashed(row, h);
        }
    }

    /// Indexes row `row`, whose (non-NULL) value hashes to `h`.
    #[inline]
    pub(crate) fn insert_hashed(&mut self, row: usize, h: u64) {
        self.entries.insert_unique(h, (h, row as u32), |&(h, _)| h);
    }

    /// Removes row `row` (still holding its indexed value) from the index.
    pub(crate) fn remove(&mut self, rows: &[Vec<Value>], row: usize) {
        let v = &rows[row][self.col];
        if v.is_null() {
            return;
        }
        let h = self.hash(v);
        if let Ok(entry) = self.entries.find_entry(h, |&(_, r)| r == row as u32) {
            entry.remove();
        }
    }

    /// The row that was at `from` is now at `to` (its value is `rows[to]`).
    pub(crate) fn moved(&mut self, rows: &[Vec<Value>], from: usize, to: usize) {
        let v = &rows[to][self.col];
        if v.is_null() {
            return;
        }
        let h = self.hash(v);
        if let Some(entry) = self.entries.find_mut(h, |&(_, r)| r == from as u32) {
            entry.1 = to as u32;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
