//! Durable storage for [`MemoryDb`](super::MemoryDb): an append-only journal.
//!
//! The file starts with a header, followed by records, one per row change,
//! written when the change commits:
//!
//! | Record | Meaning |
//! |---|---|
//! | `Create` | a table (or its new column list, when the model changed) |
//! | `Drop` | the table is gone |
//! | `Put` | insert or replace a row (by primary key) |
//! | `Del` | delete a row (by primary key) |
//! | `Seq` | the next generated id |
//!
//! Opening the file replays it. A record cut short by a crash is detected (its
//! length or checksum is wrong) and dropped, with everything after it.
//! [`compact`](super::MemoryDb::compact) rewrites the file with only the
//! current rows, then atomically replaces it; it also runs on its own when the
//! journal grows well past the data it describes.
//!
//! Each record is `[length: u32][payload][crc32: u32]`, or, in an encrypted
//! file (feature `encryption`), `[length: u32][nonce: 12 bytes][AES-256-GCM
//! ciphertext and tag]`, whose key is derived from the password with Argon2id
//! and a random salt stored in the header.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::eval::Key;
use super::{Map, State, Table};
use crate::error::{Error, Result};
use crate::schema::TableSchema;
use crate::value::Value;

const MAGIC: &[u8; 8] = b"MEMOIRE\x01";
const PLAIN: u8 = 0;
const ENCRYPTED: u8 = 1;

/// How a [`MemoryDb`](super::MemoryDb) stores its file.
#[derive(Clone, Default)]
pub struct StorageOptions {
    sync: bool,
    password: Option<String>,
}

impl std::fmt::Debug for StorageOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageOptions").field("sync", &self.sync).field("encrypted", &self.password.is_some()).finish()
    }
}

impl StorageOptions {
    /// Default options: every committed write is handed to the operating
    /// system before it returns (it survives a crash of the program; a power
    /// cut may lose the last writes), no encryption.
    pub fn new() -> Self {
        Self::default()
    }

    /// Also waits for the disk (`fsync`) after every write: it survives a power
    /// cut, at the cost of a few milliseconds per write on most disks.
    pub fn sync(mut self, yes: bool) -> Self {
        self.sync = yes;
        self
    }

    /// Encrypts the file with AES-256-GCM, with a key derived from `password`
    /// (Argon2id). Opening it again needs the same password. Feature
    /// `encryption`.
    #[cfg(feature = "encryption")]
    #[cfg_attr(docsrs, doc(cfg(feature = "encryption")))]
    pub fn password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }
}

fn io(err: std::io::Error) -> Error {
    Error::Database(Box::new(err))
}

fn corrupt(msg: impl Into<String>) -> Error {
    Error::Conversion(format!("memoire storage: {}", msg.into()))
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Record {
    Create { table: String, columns: Vec<String>, pk: usize },
    Drop { table: String },
    Put { table: String, row: Vec<Value> },
    Del { table: String, key: Value },
    Seq { table: String, next_id: i64 },
}

impl Record {
    pub(crate) fn create(schema: &TableSchema) -> Record {
        Record::Create {
            table: schema.name.to_owned(),
            columns: schema.columns.iter().map(|c| c.name.to_owned()).collect(),
            pk: schema.primary_key,
        }
    }
}

struct Enc(Vec<u8>);

impl Enc {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    fn value(&mut self, v: &Value) {
        match v {
            Value::Null => self.u8(0),
            Value::Bool(b) => {
                self.u8(1);
                self.u8(u8::from(*b));
            }
            Value::I32(i) => {
                self.u8(2);
                self.0.extend_from_slice(&i.to_le_bytes());
            }
            Value::I64(i) => {
                self.u8(3);
                self.i64(*i);
            }
            Value::F64(f) => {
                self.u8(4);
                self.0.extend_from_slice(&f.to_bits().to_le_bytes());
            }
            Value::Text(s) => {
                self.u8(5);
                self.str(s);
            }
            Value::Bytes(b) => {
                self.u8(6);
                self.bytes(b);
            }
            #[cfg(feature = "uuid")]
            Value::Uuid(u) => {
                self.u8(7);
                self.0.extend_from_slice(u.as_bytes());
            }
            #[cfg(feature = "chrono")]
            Value::Timestamp(t) => {
                self.u8(8);
                let t = t.and_utc();
                self.i64(t.timestamp());
                self.u32(t.timestamp_subsec_nanos());
            }
            #[cfg(feature = "chrono")]
            Value::TimestampTz(t) => {
                self.u8(9);
                self.i64(t.timestamp());
                self.u32(t.timestamp_subsec_nanos());
            }
            #[cfg(feature = "chrono")]
            Value::Date(d) => {
                use chrono::Datelike;
                self.u8(10);
                self.0.extend_from_slice(&d.num_days_from_ce().to_le_bytes());
            }
            #[cfg(feature = "chrono")]
            Value::Time(t) => {
                use chrono::Timelike;
                self.u8(11);
                self.u32(t.num_seconds_from_midnight());
                self.u32(t.nanosecond());
            }
            #[cfg(feature = "json")]
            Value::Json(j) => {
                self.u8(12);
                self.str(&j.to_string());
            }
        }
    }

    fn record(&mut self, r: &Record) {
        match r {
            Record::Create { table, columns, pk } => {
                self.u8(1);
                self.str(table);
                self.u32(*pk as u32);
                self.u32(columns.len() as u32);
                for c in columns {
                    self.str(c);
                }
            }
            Record::Drop { table } => {
                self.u8(2);
                self.str(table);
            }
            Record::Put { table, row } => {
                self.u8(3);
                self.str(table);
                self.u32(row.len() as u32);
                for v in row {
                    self.value(v);
                }
            }
            Record::Del { table, key } => {
                self.u8(4);
                self.str(table);
                self.value(key);
            }
            Record::Seq { table, next_id } => {
                self.u8(5);
                self.str(table);
                self.i64(*next_id);
            }
        }
    }
}

struct Dec<'a>(&'a [u8]);

impl Dec<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.0.len() < n {
            return Err(corrupt("truncated record"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap_or_default()))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap_or_default()))
    }
    fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Result<String> {
        String::from_utf8(self.bytes()?).map_err(|_| corrupt("invalid UTF-8"))
    }

    fn value(&mut self) -> Result<Value> {
        Ok(match self.u8()? {
            0 => Value::Null,
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::I32(i32::from_le_bytes(self.take(4)?.try_into().unwrap_or_default())),
            3 => Value::I64(self.i64()?),
            4 => Value::F64(f64::from_bits(u64::from_le_bytes(self.take(8)?.try_into().unwrap_or_default()))),
            5 => Value::Text(self.str()?),
            6 => Value::Bytes(self.bytes()?),
            #[cfg(feature = "uuid")]
            7 => Value::Uuid(uuid::Uuid::from_slice(self.take(16)?).map_err(|e| corrupt(e.to_string()))?),
            #[cfg(feature = "chrono")]
            8 => {
                let (s, n) = (self.i64()?, self.u32()?);
                let t = chrono::DateTime::from_timestamp(s, n).ok_or_else(|| corrupt("timestamp out of range"))?;
                Value::Timestamp(t.naive_utc())
            }
            #[cfg(feature = "chrono")]
            9 => {
                let (s, n) = (self.i64()?, self.u32()?);
                Value::TimestampTz(
                    chrono::DateTime::from_timestamp(s, n).ok_or_else(|| corrupt("timestamp out of range"))?,
                )
            }
            #[cfg(feature = "chrono")]
            10 => {
                let d = i32::from_le_bytes(self.take(4)?.try_into().unwrap_or_default());
                Value::Date(
                    chrono::NaiveDate::from_num_days_from_ce_opt(d).ok_or_else(|| corrupt("date out of range"))?,
                )
            }
            #[cfg(feature = "chrono")]
            11 => {
                let (s, n) = (self.u32()?, self.u32()?);
                Value::Time(
                    chrono::NaiveTime::from_num_seconds_from_midnight_opt(s, n)
                        .ok_or_else(|| corrupt("time out of range"))?,
                )
            }
            #[cfg(feature = "json")]
            12 => Value::Json(serde_json::from_str(&self.str()?).map_err(|e| corrupt(e.to_string()))?),
            t => {
                return Err(corrupt(format!(
                    "unknown value type {t} (was the file written with the `uuid`, `chrono` or `json` feature?)"
                )));
            }
        })
    }

    fn record(&mut self) -> Result<Record> {
        Ok(match self.u8()? {
            1 => {
                let table = self.str()?;
                let pk = self.u32()? as usize;
                let n = self.u32()? as usize;
                let columns = (0..n).map(|_| self.str()).collect::<Result<Vec<_>>>()?;
                if pk >= columns.len() {
                    return Err(corrupt("primary key out of range"));
                }
                Record::Create { table, columns, pk }
            }
            2 => Record::Drop { table: self.str()? },
            3 => {
                let table = self.str()?;
                let n = self.u32()? as usize;
                let row = (0..n).map(|_| self.value()).collect::<Result<Vec<_>>>()?;
                Record::Put { table, row }
            }
            4 => {
                let table = self.str()?;
                Record::Del { table, key: self.value()? }
            }
            5 => {
                let table = self.str()?;
                Record::Seq { table, next_id: self.i64()? }
            }
            t => return Err(corrupt(format!("unknown record type {t}"))),
        })
    }
}

/// CRC-32 (IEEE), to detect a record torn by a crash.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::LazyLock<[u32; 256]> = std::sync::LazyLock::new(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = !0u32;
    for &b in data {
        c = TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

// ---------------------------------------------------------------------------
// Encryption
// ---------------------------------------------------------------------------

#[cfg(feature = "encryption")]
mod crypto {
    use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, rand_core::RngCore};
    use aes_gcm::{Aes256Gcm, Nonce};

    use super::corrupt;
    use crate::error::Result;

    pub(super) const SALT: usize = 16;
    pub(super) const NONCE: usize = 12;
    /// Encrypted in the header to recognise a wrong password at once.
    const CHECK: &[u8] = b"memoire-storage";

    #[derive(Clone)]
    pub(crate) struct Cipher(Aes256Gcm);

    impl Cipher {
        pub(super) fn derive(password: &str, salt: &[u8]) -> Result<Cipher> {
            let mut key = [0u8; 32];
            argon2::Argon2::default()
                .hash_password_into(password.as_bytes(), salt, &mut key)
                .map_err(|e| corrupt(format!("key derivation failed: {e}")))?;
            Ok(Cipher(Aes256Gcm::new(&key.into())))
        }

        pub(super) fn salt() -> [u8; SALT] {
            let mut s = [0u8; SALT];
            OsRng.fill_bytes(&mut s);
            s
        }

        /// `nonce || ciphertext`.
        pub(super) fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
            let mut out = nonce.to_vec();
            out.extend(self.0.encrypt(&nonce, plain).map_err(|_| corrupt("encryption failed"))?);
            Ok(out)
        }

        pub(super) fn open(&self, sealed: &[u8]) -> Option<Vec<u8>> {
            if sealed.len() < NONCE {
                return None;
            }
            let (nonce, data) = sealed.split_at(NONCE);
            self.0.decrypt(Nonce::from_slice(nonce), data).ok()
        }

        pub(super) fn check_block(&self) -> Result<Vec<u8>> {
            self.seal(CHECK)
        }

        pub(super) fn verify(&self, block: &[u8]) -> bool {
            self.open(block).as_deref() == Some(CHECK)
        }
    }
}

#[cfg(not(feature = "encryption"))]
mod crypto {
    use crate::error::Result;

    #[derive(Clone)]
    pub(crate) enum Cipher {}

    impl Cipher {
        pub(super) fn seal(&self, _: &[u8]) -> Result<Vec<u8>> {
            match *self {}
        }
        pub(super) fn open(&self, _: &[u8]) -> Option<Vec<u8>> {
            match *self {}
        }
    }
}

use crypto::Cipher;

// ---------------------------------------------------------------------------
// Tables read from the file, waiting for their model
// ---------------------------------------------------------------------------

/// A table replayed from the file. It becomes a live [`Table`] the first time
/// a statement uses its model (only the model knows the column types,
/// constraints and indexes).
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub(crate) columns: Vec<String>,
    pub(crate) pk: usize,
    rows: Vec<Option<Vec<Value>>>,
    by_key: std::collections::HashMap<Key, usize>,
    live: usize,
    next_id: i64,
}

impl Pending {
    fn new(columns: Vec<String>, pk: usize) -> Self {
        Pending { columns, pk, rows: Vec::new(), by_key: Default::default(), live: 0, next_id: 1 }
    }

    pub(crate) fn len(&self) -> usize {
        self.live
    }

    /// The model changed: rows follow the new column list (by name; new
    /// columns are `NULL`).
    fn set_columns(&mut self, columns: Vec<String>, pk: usize) {
        if columns != self.columns {
            let map: Vec<Option<usize>> = columns.iter().map(|c| self.columns.iter().position(|o| o == c)).collect();
            for row in self.rows.iter_mut().flatten() {
                *row = map.iter().map(|m| m.map_or(Value::Null, |i| std::mem::take(&mut row[i]))).collect();
            }
            self.columns = columns;
        }
        if pk != self.pk {
            self.pk = pk;
            self.by_key.clear();
            for (i, row) in self.rows.iter().enumerate() {
                if let Some(r) = row {
                    self.by_key.insert(Key(r[pk].clone()), i);
                }
            }
        }
    }

    fn put(&mut self, row: Vec<Value>) -> Result<()> {
        if row.len() != self.columns.len() {
            return Err(corrupt("row width does not match its table"));
        }
        if let Some(id) = row[self.pk].as_i64() {
            self.next_id = self.next_id.max(id.saturating_add(1));
        }
        let key = Key(row[self.pk].clone());
        match self.by_key.get(&key) {
            Some(&i) => self.rows[i] = Some(row),
            None => {
                self.by_key.insert(key, self.rows.len());
                self.rows.push(Some(row));
                self.live += 1;
            }
        }
        Ok(())
    }

    fn del(&mut self, key: Value) {
        if let Some(i) = self.by_key.remove(&Key(key)) {
            self.rows[i] = None;
            self.live -= 1;
        }
    }

    /// The live table for `schema`, its rows mapped by column name.
    pub(crate) fn to_table(&self, schema: &'static TableSchema, version: u64) -> Result<Table> {
        let map: Vec<Option<usize>> =
            schema.columns.iter().map(|c| self.columns.iter().position(|o| o == c.name)).collect();
        for (c, m) in schema.columns.iter().zip(&map) {
            if m.is_none() && !c.nullable && self.live > 0 {
                return Err(Error::Conversion(format!(
                    "`{}.{}` is not in the stored table and is not nullable: make the field an `Option`, \
                     or migrate the data",
                    schema.name, c.name
                )));
            }
        }
        let mut t = Table::new(schema, version);
        t.rows.reserve(self.live);
        for row in self.rows.iter().flatten() {
            t.insert(map.iter().map(|m| m.map_or(Value::Null, |i| row[i].clone())).collect(), false)?;
        }
        t.next_id = t.next_id.max(self.next_id);
        Ok(t)
    }

    /// `true` when `schema` lists other columns (or another key) than the file.
    pub(crate) fn differs(&self, schema: &TableSchema) -> bool {
        self.pk != schema.primary_key
            || self.columns.len() != schema.columns.len()
            || self.columns.iter().zip(schema.columns).any(|(a, b)| a != b.name)
    }

    fn records(&self, table: &str, out: &mut Vec<Record>) {
        out.push(Record::Create { table: table.to_owned(), columns: self.columns.clone(), pk: self.pk });
        out.push(Record::Seq { table: table.to_owned(), next_id: self.next_id });
        for row in self.rows.iter().flatten() {
            out.push(Record::Put { table: table.to_owned(), row: row.clone() });
        }
    }
}

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

pub(crate) struct Store {
    path: PathBuf,
    file: File,
    cipher: Option<Cipher>,
    sync: bool,
    /// Header written at the top of the file (kept for compaction).
    header: Vec<u8>,
    /// Records written since the file was last rewritten.
    records: u64,
    /// A write to the file failed: the file no longer matches the memory.
    failed: bool,
}

impl Store {
    /// Opens or creates the file and replays it into tables waiting for their
    /// model.
    pub(crate) fn open(path: &Path, options: &StorageOptions) -> Result<(Store, Map<String, Arc<Pending>>)> {
        let mut file = OpenOptions::new().read(true).append(true).create(true).open(path).map_err(io)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(io)?;
        let (header, cipher, start) = if data.is_empty() {
            let (header, cipher) = new_header(options)?;
            file.write_all(&header).map_err(io)?;
            file.sync_all().map_err(io)?;
            let len = header.len();
            (header, cipher, len)
        } else {
            read_header(&data, options)?
        };
        let mut store = Store {
            path: path.to_owned(),
            file,
            cipher,
            sync: options.sync,
            header: header.clone(),
            records: 0,
            failed: false,
        };
        let mut tables: Map<String, Pending> = Map::default();
        let mut at = start;
        while at < data.len() {
            // A frame cut short or failing its checksum is a torn tail; a sound
            // frame that cannot be decoded is an error (never silently dropped).
            let Some((payload, next)) = store.read_frame(&data, at) else { break };
            let record = Dec(&payload).record()?;
            apply(&mut tables, record)?;
            store.records += 1;
            at = next;
        }
        if at < data.len() {
            // A torn or damaged tail: drop it so new records follow the last good one.
            store.file.set_len(at as u64).map_err(io)?;
            store.file.sync_all().map_err(io)?;
        }
        Ok((store, tables.into_iter().map(|(k, v)| (k, Arc::new(v))).collect()))
    }

    /// The payload of the record at `at`, and where the next one starts.
    fn read_frame(&self, data: &[u8], at: usize) -> Option<(Vec<u8>, usize)> {
        let len = u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize;
        let body = data.get(at + 4..at + 4 + len)?;
        match &self.cipher {
            Some(c) => Some((c.open(body)?, at + 4 + len)),
            None => {
                let crc = u32::from_le_bytes(data.get(at + 4 + len..at + 8 + len)?.try_into().ok()?);
                (crc32(body) == crc).then(|| (body.to_vec(), at + 8 + len))
            }
        }
    }

    fn frame(&self, records: &[Record], out: &mut Vec<u8>) -> Result<()> {
        for r in records {
            let mut enc = Enc(Vec::with_capacity(64));
            enc.record(r);
            match &self.cipher {
                Some(c) => {
                    let sealed = c.seal(&enc.0)?;
                    out.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
                    out.extend_from_slice(&sealed);
                }
                None => {
                    out.extend_from_slice(&(enc.0.len() as u32).to_le_bytes());
                    out.extend_from_slice(&enc.0);
                    out.extend_from_slice(&crc32(&enc.0).to_le_bytes());
                }
            }
        }
        Ok(())
    }

    /// Appends records (one committed statement or transaction) in one write.
    pub(crate) fn append(&mut self, records: &[Record]) -> Result<()> {
        if self.failed {
            return Err(corrupt("an earlier write to the file failed; reopen the database"));
        }
        if records.is_empty() {
            return Ok(());
        }
        let mut buf = Vec::with_capacity(records.len() * 64);
        self.frame(records, &mut buf)?;
        let written = self.file.write_all(&buf).and_then(|()| if self.sync { self.file.sync_data() } else { Ok(()) });
        if let Err(e) = written {
            self.failed = true;
            return Err(io(e));
        }
        self.records += records.len() as u64;
        Ok(())
    }

    /// `true` when the journal holds much more than the data it describes.
    pub(crate) fn should_compact(&self, live_rows: usize) -> bool {
        self.records > 10_000 && self.records > 4 * live_rows as u64
    }

    /// Rewrites the file with the current content, then replaces it atomically.
    pub(crate) fn compact(&mut self, state: &State) -> Result<()> {
        let mut records = Vec::new();
        let mut names: Vec<&String> = state.tables.keys().chain(state.pending.keys()).collect();
        names.sort();
        for name in names {
            if let Some(t) = state.tables.get(name) {
                records.push(Record::create(t.schema));
                records.push(Record::Seq { table: name.clone(), next_id: t.next_id });
                for row in &t.rows {
                    records.push(Record::Put { table: name.clone(), row: row.clone() });
                }
            } else if let Some(p) = state.pending.get(name) {
                p.records(name, &mut records);
            }
        }
        let mut tmp_name = self.path.as_os_str().to_owned();
        tmp_name.push(".compact");
        let tmp = PathBuf::from(tmp_name);
        let mut buf = self.header.clone();
        self.frame(&records, &mut buf)?;
        {
            let mut f = File::create(&tmp).map_err(io)?;
            f.write_all(&buf).map_err(io)?;
            f.sync_all().map_err(io)?;
        }
        fs::rename(&tmp, &self.path).map_err(io)?;
        self.file = OpenOptions::new().append(true).open(&self.path).map_err(io)?;
        self.records = records.len() as u64;
        self.failed = false;
        Ok(())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

fn apply(tables: &mut Map<String, Pending>, record: Record) -> Result<()> {
    match record {
        Record::Create { table, columns, pk } => match tables.get_mut(&table) {
            Some(t) => t.set_columns(columns, pk),
            None => {
                tables.insert(table, Pending::new(columns, pk));
            }
        },
        Record::Drop { table } => {
            tables.remove(&table);
        }
        Record::Put { table, row } => {
            tables.get_mut(&table).ok_or_else(|| corrupt(format!("row for unknown table `{table}`")))?.put(row)?;
        }
        Record::Del { table, key } => {
            if let Some(t) = tables.get_mut(&table) {
                t.del(key);
            }
        }
        Record::Seq { table, next_id } => {
            if let Some(t) = tables.get_mut(&table) {
                t.next_id = t.next_id.max(next_id);
            }
        }
    }
    Ok(())
}

fn new_header(options: &StorageOptions) -> Result<(Vec<u8>, Option<Cipher>)> {
    let mut header = MAGIC.to_vec();
    match &options.password {
        None => {
            header.push(PLAIN);
            Ok((header, None))
        }
        #[cfg(feature = "encryption")]
        Some(password) => {
            header.push(ENCRYPTED);
            let salt = Cipher::salt();
            let cipher = Cipher::derive(password, &salt)?;
            header.extend_from_slice(&salt);
            let check = cipher.check_block()?;
            header.extend_from_slice(&(check.len() as u32).to_le_bytes());
            header.extend_from_slice(&check);
            Ok((header, Some(cipher)))
        }
        #[cfg(not(feature = "encryption"))]
        Some(_) => Err(Error::Unsupported("encryption needs the `encryption` feature".into())),
    }
}

/// `(header, cipher, offset of the first record)`.
fn read_header(data: &[u8], options: &StorageOptions) -> Result<(Vec<u8>, Option<Cipher>, usize)> {
    if data.len() < MAGIC.len() + 1 || &data[..MAGIC.len()] != MAGIC {
        return Err(corrupt("not a Memoire storage file"));
    }
    match data[MAGIC.len()] {
        PLAIN => {
            if options.password.is_some() {
                return Err(corrupt("the file is not encrypted, but a password was given"));
            }
            let end = MAGIC.len() + 1;
            Ok((data[..end].to_vec(), None, end))
        }
        #[cfg(feature = "encryption")]
        ENCRYPTED => {
            let Some(password) = &options.password else {
                return Err(corrupt("the file is encrypted: open it with `StorageOptions::password`"));
            };
            let at = MAGIC.len() + 1;
            let salt = data.get(at..at + crypto::SALT).ok_or_else(|| corrupt("truncated header"))?;
            let len_at = at + crypto::SALT;
            let len = u32::from_le_bytes(
                data.get(len_at..len_at + 4).ok_or_else(|| corrupt("truncated header"))?.try_into().unwrap_or_default(),
            ) as usize;
            let check = data.get(len_at + 4..len_at + 4 + len).ok_or_else(|| corrupt("truncated header"))?;
            let cipher = Cipher::derive(password, salt)?;
            if !cipher.verify(check) {
                return Err(Error::Conversion("memoire storage: wrong password".into()));
            }
            let end = len_at + 4 + len;
            Ok((data[..end].to_vec(), Some(cipher), end))
        }
        #[cfg(not(feature = "encryption"))]
        ENCRYPTED => Err(Error::Unsupported("the file is encrypted: enable the `encryption` feature".into())),
        v => Err(corrupt(format!("unknown storage format {v}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let records = vec![
            Record::Create { table: "t".into(), columns: vec!["id".into(), "name".into()], pk: 0 },
            Record::Put {
                table: "t".into(),
                row: vec![Value::I64(-3), Value::Text("é\u{0}x".into()), Value::F64(1.5), Value::Bytes(vec![0, 255])],
            },
            Record::Del { table: "t".into(), key: Value::I32(7) },
            Record::Seq { table: "t".into(), next_id: 42 },
            Record::Drop { table: "t".into() },
        ];
        for r in records {
            let mut e = Enc(Vec::new());
            e.record(&r);
            assert_eq!(Dec(&e.0).record().unwrap(), r);
        }
    }

    #[cfg(all(feature = "chrono", feature = "uuid", feature = "json"))]
    #[test]
    fn typed_values_round_trip() {
        use chrono::{NaiveDate, TimeZone, Utc};
        let ts = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap().and_hms_nano_opt(13, 14, 15, 123_456_789).unwrap();
        let values = vec![
            Value::Uuid(uuid::Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0)),
            Value::Timestamp(ts),
            Value::TimestampTz(Utc.from_utc_datetime(&ts)),
            Value::Date(ts.date()),
            Value::Time(ts.time()),
            Value::Json(serde_json::json!({"a": [1, 2.5, null], "b": "é"})),
            Value::Bool(true),
            Value::I32(-7),
        ];
        let r = Record::Put { table: "t".into(), row: values };
        let mut e = Enc(Vec::new());
        e.record(&r);
        assert_eq!(Dec(&e.0).record().unwrap(), r);
    }

    #[test]
    fn crc_matches_the_reference() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
