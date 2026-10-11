//! Hash indexes that store row positions only. A lookup hashes the probed
//! value and compares it with the value in the row, so keys are never copied:
//! inserting a row with a unique `email` costs no allocation for its index.

use std::hash::{BuildHasher, Hash, Hasher};

use foldhash::fast::RandomState;
use hashbrown::HashTable;

use crate::value::Value;

use super::eval::{KeyRef, key_eq};

#[derive(Clone)]
pub(crate) struct Index {
    /// Indexed columns: one, or several for `#[memoire(index(a, b))]`.
    pub(crate) cols: Box<[usize]>,
    /// Primary key or `unique`: at most one row per value.
    pub(crate) unique: bool,
    hasher: RandomState,
    /// `(hash of the key, row)`; keys holding a NULL are not indexed.
    entries: HashTable<(u64, u32)>,
}

impl Index {
    pub(crate) fn new(cols: Vec<usize>, unique: bool) -> Self {
        Index { cols: cols.into_boxed_slice(), unique, hasher: RandomState::default(), entries: HashTable::new() }
    }

    /// The column, for a single-column index.
    #[inline]
    pub(crate) fn single(&self) -> Option<usize> {
        match *self.cols {
            [c] => Some(c),
            _ => None,
        }
    }

    /// The hash of a key given as values, in column order.
    #[inline]
    fn hash_values<'v>(&self, values: impl IntoIterator<Item = &'v Value>) -> u64 {
        let mut state = self.hasher.build_hasher();
        for v in values {
            KeyRef(v).hash(&mut state);
        }
        state.finish()
    }

    /// The hash this index files a one-column value `v` under.
    #[inline]
    pub(crate) fn hash(&self, v: &Value) -> u64 {
        self.hash_values([v])
    }

    /// The hash of the key of `row`; `None` when one of its columns is NULL
    /// (such rows are not indexed and never violate a unique index).
    #[inline]
    pub(crate) fn row_hash(&self, row: &[Value]) -> Option<u64> {
        if self.cols.iter().any(|&c| row[c].is_null()) {
            return None;
        }
        Some(self.hash_values(self.cols.iter().map(|&c| &row[c])))
    }

    #[inline]
    fn same_key(&self, stored: &[Value], probe: &[Value]) -> bool {
        self.cols.iter().all(|&c| key_eq(&stored[c], &probe[c]))
    }

    /// A row whose value equals `v` (the only one for a unique index), for a
    /// single-column index.
    #[inline]
    pub(crate) fn find(&self, rows: &[Vec<Value>], v: &Value) -> Option<usize> {
        if v.is_null() {
            return None;
        }
        let col = self.cols[0];
        let h = self.hash(v);
        self.entries.find(h, |&(eh, r)| eh == h && key_eq(&rows[r as usize][col], v)).map(|&(_, r)| r as usize)
    }

    /// A stored row with the same key as `row`, whose key hashes to `h`.
    #[inline]
    pub(crate) fn find_row_hashed(&self, rows: &[Vec<Value>], row: &[Value], h: u64) -> Option<usize> {
        self.entries.find(h, |&(eh, r)| eh == h && self.same_key(&rows[r as usize], row)).map(|&(_, r)| r as usize)
    }

    /// Every row whose value equals `v`, in table order (single-column index).
    pub(crate) fn find_all(&self, rows: &[Vec<Value>], v: &Value) -> Vec<usize> {
        self.find_all_values(rows, &[v])
    }

    /// Every row whose key equals `values` (one per indexed column, in order),
    /// in table order.
    pub(crate) fn find_all_values(&self, rows: &[Vec<Value>], values: &[&Value]) -> Vec<usize> {
        if values.len() != self.cols.len() || values.iter().any(|v| v.is_null()) {
            return Vec::new();
        }
        let h = self.hash_values(values.iter().copied());
        let matches = |r: u32| self.cols.iter().zip(values).all(|(&c, v)| key_eq(&rows[r as usize][c], v));
        let mut out: Vec<usize> =
            self.entries.iter_hash(h).filter(|&&(eh, r)| eh == h && matches(r)).map(|&(_, r)| r as usize).collect();
        out.sort_unstable();
        out
    }

    /// Indexes row `row` (already stored in `rows`).
    #[inline]
    pub(crate) fn insert(&mut self, rows: &[Vec<Value>], row: usize) {
        if let Some(h) = self.row_hash(&rows[row]) {
            self.insert_hashed(row, h);
        }
    }

    /// Indexes row `row`, whose key (without NULL) hashes to `h`.
    #[inline]
    pub(crate) fn insert_hashed(&mut self, row: usize, h: u64) {
        self.entries.insert_unique(h, (h, row as u32), |&(h, _)| h);
    }

    /// Removes row `row` (still holding its indexed values) from the index.
    pub(crate) fn remove(&mut self, rows: &[Vec<Value>], row: usize) {
        let Some(h) = self.row_hash(&rows[row]) else { return };
        if let Ok(entry) = self.entries.find_entry(h, |&(_, r)| r == row as u32) {
            entry.remove();
        }
    }

    /// The row that was at `from` is now at `to` (its values are `rows[to]`).
    pub(crate) fn moved(&mut self, rows: &[Vec<Value>], from: usize, to: usize) {
        let Some(h) = self.row_hash(&rows[to]) else { return };
        if let Some(entry) = self.entries.find_mut(h, |&(_, r)| r == from as u32) {
            entry.1 = to as u32;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Word index of a `#[memoire(fulltext)]` column: one entry per distinct word
/// of each row. A search intersects the rows of its words; hash collisions can
/// only add candidates, which the search itself then rejects.
#[derive(Clone)]
pub(crate) struct WordIndex {
    pub(crate) col: usize,
    hasher: RandomState,
    /// `(hash of a lower-case word, row)`.
    entries: HashTable<(u64, u32)>,
}

impl WordIndex {
    pub(crate) fn new(col: usize) -> Self {
        WordIndex { col, hasher: RandomState::default(), entries: HashTable::new() }
    }

    fn word_hashes(&self, row: &[Value]) -> Vec<u64> {
        let Value::Text(text) = &row[self.col] else { return Vec::new() };
        let mut hashes: Vec<u64> = crate::expr::text_words(text).map(|w| self.hasher.hash_one(w.as_str())).collect();
        hashes.sort_unstable();
        hashes.dedup();
        hashes
    }

    pub(crate) fn insert(&mut self, rows: &[Vec<Value>], row: usize) {
        for h in self.word_hashes(&rows[row]) {
            self.entries.insert_unique(h, (h, row as u32), |&(h, _)| h);
        }
    }

    /// Removes row `row` (still holding its text).
    pub(crate) fn remove(&mut self, rows: &[Vec<Value>], row: usize) {
        for h in self.word_hashes(&rows[row]) {
            if let Ok(entry) = self.entries.find_entry(h, |&(eh, r)| eh == h && r == row as u32) {
                entry.remove();
            }
        }
    }

    /// The row that was at `from` is now at `to`.
    pub(crate) fn moved(&mut self, rows: &[Vec<Value>], from: usize, to: usize) {
        for h in self.word_hashes(&rows[to]) {
            if let Some(entry) = self.entries.find_mut(h, |&(eh, r)| eh == h && r == from as u32) {
                entry.1 = to as u32;
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    /// Rows that may contain every one of `words` (sorted, deduplicated).
    pub(crate) fn candidates(&self, words: &[String]) -> Vec<usize> {
        let mut out: Option<Vec<usize>> = None;
        for w in words {
            let h = self.hasher.hash_one(w.as_str());
            let mut rows: Vec<usize> =
                self.entries.iter_hash(h).filter(|&&(eh, _)| eh == h).map(|&(_, r)| r as usize).collect();
            rows.sort_unstable();
            rows.dedup();
            out = Some(match out {
                None => rows,
                Some(prev) => {
                    let mut kept = Vec::with_capacity(prev.len().min(rows.len()));
                    let (mut i, mut j) = (0, 0);
                    while i < prev.len() && j < rows.len() {
                        match prev[i].cmp(&rows[j]) {
                            std::cmp::Ordering::Less => i += 1,
                            std::cmp::Ordering::Greater => j += 1,
                            std::cmp::Ordering::Equal => {
                                kept.push(prev[i]);
                                i += 1;
                                j += 1;
                            }
                        }
                    }
                    kept
                }
            });
            if out.as_ref().is_some_and(Vec::is_empty) {
                break;
            }
        }
        out.unwrap_or_default()
    }
}
