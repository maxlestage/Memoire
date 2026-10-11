//! Hash indexes that store row positions only. A lookup hashes the probed
//! value and compares it with the value in the row, so keys are never copied:
//! inserting a row with a unique `email` costs no allocation for its index.

use std::hash::{BuildHasher, Hash, Hasher};

use foldhash::fast::RandomState;
use hashbrown::HashTable;

use crate::value::Value;

use super::eval::{KeyRef, key_eq};

type RowSet = hashbrown::HashSet<u32, RandomState>;
type Groups = hashbrown::HashMap<u64, Group, RandomState>;

/// The rows filed under one key hash of a non-unique index: most keys hold a
/// single row, kept without an allocation.
#[derive(Clone)]
enum Group {
    One(u32),
    Many(RowSet),
}

impl Group {
    fn insert(&mut self, row: u32) {
        match self {
            Group::One(r) if *r == row => {}
            Group::One(r) => {
                let mut set = RowSet::default();
                set.insert(*r);
                set.insert(row);
                *self = Group::Many(set);
            }
            Group::Many(set) => {
                set.insert(row);
            }
        }
    }

    /// Removes `row`; `true` when the group is now empty.
    fn remove(&mut self, row: u32) -> bool {
        match self {
            Group::One(r) => *r == row,
            Group::Many(set) => {
                set.remove(&row);
                if set.len() == 1
                    && let Some(&only) = set.iter().next()
                {
                    *self = Group::One(only);
                }
                false
            }
        }
    }

    fn replace(&mut self, from: u32, to: u32) {
        match self {
            Group::One(r) if *r == from => *r = to,
            Group::One(_) => {}
            Group::Many(set) => {
                if set.remove(&from) {
                    set.insert(to);
                }
            }
        }
    }

    fn iter(&self) -> Box<dyn Iterator<Item = u32> + '_> {
        match self {
            Group::One(r) => Box::new(std::iter::once(*r)),
            Group::Many(set) => Box::new(set.iter().copied()),
        }
    }
}

/// Where an index files its rows.
#[derive(Clone)]
enum Entries {
    /// Unique index: `(hash of the key, row)`, at most one row per key.
    Unique(HashTable<(u64, u32)>),
    /// Non-unique index: the rows of each key hash. Many rows may share a key
    /// (an index on a `bool` column), so they live in a set rather than in a
    /// probe sequence that would grow with them.
    Multi(Groups),
}

#[derive(Clone)]
pub(crate) struct Index {
    /// Indexed columns: one, or several for `#[memoire(index(a, b))]`.
    pub(crate) cols: Box<[usize]>,
    /// Primary key or `unique`: at most one row per value.
    pub(crate) unique: bool,
    hasher: RandomState,
    /// Keys holding a NULL are not indexed.
    entries: Entries,
}

impl Index {
    pub(crate) fn new(cols: Vec<usize>, unique: bool) -> Self {
        let entries = if unique { Entries::Unique(HashTable::new()) } else { Entries::Multi(Groups::default()) };
        Index { cols: cols.into_boxed_slice(), unique, hasher: RandomState::default(), entries }
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

    /// Rows filed under hash `h` (all of them for a non-unique index).
    fn under(&self, h: u64) -> Box<dyn Iterator<Item = u32> + '_> {
        match &self.entries {
            Entries::Unique(t) => Box::new(t.iter_hash(h).filter(move |&&(eh, _)| eh == h).map(|&(_, r)| r)),
            Entries::Multi(g) => match g.get(&h) {
                Some(group) => group.iter(),
                None => Box::new(std::iter::empty()),
            },
        }
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
        if let Entries::Unique(t) = &self.entries {
            return t.find(h, |&(eh, r)| eh == h && key_eq(&rows[r as usize][col], v)).map(|&(_, r)| r as usize);
        }
        self.under(h).find(|&r| key_eq(&rows[r as usize][col], v)).map(|r| r as usize)
    }

    /// A stored row with the same key as `row`, whose key hashes to `h`.
    #[inline]
    pub(crate) fn find_row_hashed(&self, rows: &[Vec<Value>], row: &[Value], h: u64) -> Option<usize> {
        if let Entries::Unique(t) = &self.entries {
            return t.find(h, |&(eh, r)| eh == h && self.same_key(&rows[r as usize], row)).map(|&(_, r)| r as usize);
        }
        self.under(h).find(|&r| self.same_key(&rows[r as usize], row)).map(|r| r as usize)
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
        let mut out: Vec<usize> = self.under(h).filter(|&r| matches(r)).map(|r| r as usize).collect();
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
        match &mut self.entries {
            Entries::Unique(t) => {
                t.insert_unique(h, (h, row as u32), |&(h, _)| h);
            }
            Entries::Multi(g) => match g.get_mut(&h) {
                Some(group) => group.insert(row as u32),
                None => {
                    g.insert(h, Group::One(row as u32));
                }
            },
        }
    }

    /// Removes row `row` (still holding its indexed values) from the index.
    pub(crate) fn remove(&mut self, rows: &[Vec<Value>], row: usize) {
        let Some(h) = self.row_hash(&rows[row]) else { return };
        match &mut self.entries {
            Entries::Unique(t) => {
                if let Ok(entry) = t.find_entry(h, |&(_, r)| r == row as u32) {
                    entry.remove();
                }
            }
            Entries::Multi(g) => {
                if g.get_mut(&h).is_some_and(|group| group.remove(row as u32)) {
                    g.remove(&h);
                }
            }
        }
    }

    /// The row that was at `from` is now at `to` (its values are `rows[to]`).
    pub(crate) fn moved(&mut self, rows: &[Vec<Value>], from: usize, to: usize) {
        let Some(h) = self.row_hash(&rows[to]) else { return };
        match &mut self.entries {
            Entries::Unique(t) => {
                if let Some(entry) = t.find_mut(h, |&(_, r)| r == from as u32) {
                    entry.1 = to as u32;
                }
            }
            Entries::Multi(g) => {
                if let Some(group) = g.get_mut(&h) {
                    group.replace(from as u32, to as u32);
                }
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        match &mut self.entries {
            Entries::Unique(t) => t.clear(),
            Entries::Multi(g) => g.clear(),
        }
    }
}

/// Word index of a `#[memoire(fulltext)]` column: for each word (by hash),
/// the set of rows holding it. A search starts from its rarest word and keeps
/// the rows found under every other word; hash collisions can only add
/// candidates, which the search itself then rejects.
#[derive(Clone)]
pub(crate) struct WordIndex {
    pub(crate) col: usize,
    hasher: RandomState,
    postings: hashbrown::HashMap<u64, hashbrown::HashSet<u32, RandomState>, RandomState>,
}

impl WordIndex {
    pub(crate) fn new(col: usize) -> Self {
        WordIndex { col, hasher: RandomState::default(), postings: hashbrown::HashMap::default() }
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
            self.postings.entry(h).or_default().insert(row as u32);
        }
    }

    /// Removes row `row` (still holding its text).
    pub(crate) fn remove(&mut self, rows: &[Vec<Value>], row: usize) {
        for h in self.word_hashes(&rows[row]) {
            if let Some(set) = self.postings.get_mut(&h) {
                set.remove(&(row as u32));
                if set.is_empty() {
                    self.postings.remove(&h);
                }
            }
        }
    }

    /// The row that was at `from` is now at `to`.
    pub(crate) fn moved(&mut self, rows: &[Vec<Value>], from: usize, to: usize) {
        for h in self.word_hashes(&rows[to]) {
            if let Some(set) = self.postings.get_mut(&h)
                && set.remove(&(from as u32))
            {
                set.insert(to as u32);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.postings.clear();
    }

    /// Rows that may contain every one of `words`, in table order.
    pub(crate) fn candidates(&self, words: &[String]) -> Vec<usize> {
        let mut sets = Vec::with_capacity(words.len());
        for w in words {
            match self.postings.get(&self.hasher.hash_one(w.as_str())) {
                Some(set) => sets.push(set),
                None => return Vec::new(),
            }
        }
        sets.sort_by_key(|s| s.len());
        let Some((rarest, others)) = sets.split_first() else { return Vec::new() };
        let mut out: Vec<usize> =
            rarest.iter().filter(|r| others.iter().all(|s| s.contains(*r))).map(|&r| r as usize).collect();
        out.sort_unstable();
        out
    }
}
