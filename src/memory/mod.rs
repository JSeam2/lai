//! Local, source-linked memory storage.
pub mod embedding;
use std::{fs, path::Path, time::Duration};

use anyhow::{Context, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct NewMemory {
    pub text: String,
    pub source: String,
    pub supersedes: Option<i64>,
}
impl NewMemory {
    pub fn new(text: &str, source: &str) -> Self {
        Self {
            text: text.into(),
            source: source.into(),
            supersedes: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub id: i64,
    pub text: String,
    pub source: String,
    pub created_at: String,
    pub supersedes: Option<i64>,
}

pub struct Job {
    pub id: i64,
    pub text: String,
}
pub struct MemoryStore {
    conn: Connection,
    vector_dimension: Option<usize>,
    vector_model: Option<String>,
}

impl MemoryStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("cannot open memory database {}", path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= 3,
            "memory database was created by a newer version of LAI"
        );
        if version == 0 {
            conn.execute_batch("BEGIN;
                CREATE TABLE memories (id INTEGER PRIMARY KEY, text TEXT NOT NULL, source TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')), supersedes INTEGER REFERENCES memories(id) ON DELETE SET NULL, UNIQUE(text, source));
                CREATE TABLE jobs (id INTEGER PRIMARY KEY, digest BLOB NOT NULL UNIQUE, text TEXT NOT NULL);
                CREATE TABLE processed (digest BLOB PRIMARY KEY);
                PRAGMA user_version = 1; COMMIT;")?;
        }
        if version < 2 {
            conn.execute_batch("BEGIN;
                CREATE TABLE memory_embeddings (memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE, embedding BLOB NOT NULL);
                CREATE TABLE embedding_metadata (id INTEGER PRIMARY KEY CHECK(id=1), model TEXT NOT NULL, dimension INTEGER NOT NULL);
                PRAGMA user_version = 2; COMMIT;")?;
        }
        if version < 3 {
            conn.execute_batch(
                "BEGIN;
                DROP TRIGGER IF EXISTS memories_ai;
                DROP TRIGGER IF EXISTS memories_ad;
                DROP TABLE IF EXISTS memories_fts;
                PRAGMA user_version = 3; COMMIT;",
            )?;
        }
        Ok(Self {
            conn,
            vector_dimension: None,
            vector_model: None,
        })
    }
    pub fn insert(&mut self, memory: &NewMemory) -> anyhow::Result<i64> {
        validate(memory)?;
        self.conn.execute("INSERT INTO memories(text,source,supersedes) VALUES(?1,?2,?3) ON CONFLICT(text,source) DO NOTHING", params![memory.text,memory.source,memory.supersedes])?;
        Ok(self.conn.query_row(
            "SELECT id FROM memories WHERE text=?1 AND source=?2",
            params![memory.text, memory.source],
            |r| r.get(0),
        )?)
    }
    pub fn get(&self, id: i64) -> anyhow::Result<Option<Memory>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id,text,source,created_at,supersedes FROM memories WHERE id=?1",
                [id],
                row,
            )
            .optional()?)
    }
    pub fn list(&self) -> anyhow::Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,text,source,created_at,supersedes FROM memories ORDER BY id DESC",
        )?;
        Ok(stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?)
    }
    /// Configure the embedding space on this connection. A changed model invalidates old vectors.
    pub fn configure_vectors(&mut self, model: &str, dimension: usize) -> anyhow::Result<()> {
        ensure!(
            !model.trim().is_empty() && (1..=65536).contains(&dimension),
            "invalid embedding model or dimension"
        );
        if let Some(current) = self.vector_dimension {
            ensure!(
                current == dimension,
                "changing embedding dimensions requires reopening the database"
            );
        } else {
            register_vector_extension(&self.conn)?;
            let options = format!("dimension={dimension},type=FLOAT32,distance=COSINE");
            self.conn.query_row(
                "SELECT vector_init('memory_embeddings','embedding',?1)",
                [&options],
                |_| Ok(()),
            )?;
            self.vector_dimension = Some(dimension);
        }
        let previous: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT model,dimension FROM embedding_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if previous
            .as_ref()
            .is_none_or(|(old, size)| old != model || *size != dimension as i64)
        {
            let tx = self.conn.transaction()?;
            tx.execute("DELETE FROM memory_embeddings", [])?;
            tx.execute("INSERT INTO embedding_metadata(id,model,dimension) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET model=excluded.model,dimension=excluded.dimension", params![model, dimension as i64])?;
            tx.commit()?;
        }
        self.vector_model = Some(model.to_owned());
        Ok(())
    }
    pub fn put_embedding(&self, id: i64, embedding: &[f32]) -> anyhow::Result<()> {
        let blob = self.vector_blob(embedding)?;
        let changed = self.conn.execute("INSERT INTO memory_embeddings(memory_id,embedding) SELECT ?1,?2 WHERE EXISTS (SELECT 1 FROM embedding_metadata WHERE id=1 AND model=?3 AND dimension=?4) ON CONFLICT(memory_id) DO UPDATE SET embedding=excluded.embedding", params![id,blob,self.vector_model,embedding.len() as i64])?;
        ensure!(
            changed == 1,
            "embedding model changed; reopen the memory database"
        );
        Ok(())
    }
    pub fn has_embeddings(&self) -> anyhow::Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT EXISTS(SELECT 1 FROM memory_embeddings)", [], |r| {
                r.get(0)
            })?)
    }
    pub fn unembedded(&self, limit: usize) -> anyhow::Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare("SELECT m.id,m.text,m.source,m.created_at,m.supersedes FROM memories m LEFT JOIN memory_embeddings e ON e.memory_id=m.id WHERE e.memory_id IS NULL ORDER BY m.id LIMIT ?1")?;
        Ok(stmt
            .query_map([limit.min(100) as i64], row)?
            .collect::<Result<Vec<_>, _>>()?)
    }
    pub fn search_vectors(&self, embedding: &[f32], limit: usize) -> anyhow::Result<Vec<Memory>> {
        let blob = self.vector_blob(embedding)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare("SELECT m.id,m.text,m.source,m.created_at,m.supersedes FROM vector_full_scan('memory_embeddings','embedding',?1,?2) v JOIN memories m ON m.id=v.rowid ORDER BY v.distance")?;
        Ok(stmt
            .query_map(params![blob, limit.min(100) as i64], row)?
            .collect::<Result<Vec<_>, _>>()?)
    }
    fn vector_blob(&self, embedding: &[f32]) -> anyhow::Result<Vec<u8>> {
        ensure!(
            Some(embedding.len()) == self.vector_dimension,
            "incorrect embedding dimension or vector search is not configured"
        );
        let current: Option<String> = self
            .conn
            .query_row("SELECT model FROM embedding_metadata WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(
            current.is_some() && current == self.vector_model,
            "embedding model changed; reopen the memory database"
        );
        ensure!(
            embedding.iter().all(|v| v.is_finite()) && embedding.iter().any(|v| *v != 0.0),
            "embedding must contain finite non-zero values"
        );
        Ok(embedding.iter().flat_map(|v| v.to_ne_bytes()).collect())
    }
    pub fn enqueue(&self, text: &str) -> anyhow::Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        let digest = Sha256::digest(text.as_bytes());
        self.conn.execute("INSERT INTO jobs(digest,text) SELECT ?1,?2 WHERE NOT EXISTS (SELECT 1 FROM processed WHERE digest=?1) ON CONFLICT(digest) DO NOTHING", params![digest.as_slice(),text])?;
        Ok(())
    }
    pub fn next_job(&self) -> anyhow::Result<Option<Job>> {
        Ok(self
            .conn
            .query_row("SELECT id,text FROM jobs ORDER BY id LIMIT 1", [], |r| {
                Ok(Job {
                    id: r.get(0)?,
                    text: r.get(1)?,
                })
            })
            .optional()?)
    }
    pub fn finish_job(&mut self, id: i64, memories: &[NewMemory]) -> anyhow::Result<()> {
        for memory in memories {
            validate(memory)?;
        }
        let tx = self.conn.transaction()?;
        ensure!(
            tx.execute(
                "INSERT INTO processed SELECT digest FROM jobs WHERE id=?1",
                [id]
            )? == 1,
            "memory job no longer exists"
        );
        for memory in memories {
            tx.execute("INSERT INTO memories(text,source,supersedes) VALUES(?1,?2,?3) ON CONFLICT(text,source) DO NOTHING", params![memory.text,memory.source,memory.supersedes])?;
        }
        tx.execute("DELETE FROM jobs WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete(&mut self, id: i64) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM memories WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.conn.execute_batch(
            "BEGIN; DELETE FROM memories; DELETE FROM jobs; DELETE FROM processed; COMMIT;",
        )?;
        Ok(())
    }
}
fn validate(memory: &NewMemory) -> anyhow::Result<()> {
    ensure!(
        !memory.text.trim().is_empty() && !memory.source.trim().is_empty(),
        "memory and source must not be empty"
    );
    Ok(())
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        text: r.get(1)?,
        source: r.get(2)?,
        created_at: r.get(3)?,
        supersedes: r.get(4)?,
    })
}

/// Validate model-produced memories against their user-provided evidence.
pub fn parse_extraction(output: &str, source: &str) -> anyhow::Result<Vec<NewMemory>> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Candidate {
        text: String,
        source: String,
    }
    let candidates: Vec<Candidate> = serde_json::from_str(output)?;
    ensure!(candidates.len() <= 16, "too many extracted memories");
    candidates
        .into_iter()
        .map(|candidate| {
            ensure!(
                !candidate.source.trim().is_empty() && source.contains(&candidate.source),
                "memory source is not present in user text"
            );
            ensure!(
                candidate.text.len() <= 1024 && candidate.source.len() <= 2048,
                "extracted memory is too long"
            );
            let memory = NewMemory::new(&candidate.text, &candidate.source);
            validate(&memory)?;
            Ok(memory)
        })
        .collect()
}

unsafe extern "C" {
    fn sqlite3_vector_init(
        db: *mut rusqlite::ffi::sqlite3,
        error: *mut *mut std::ffi::c_char,
        api: *const std::ffi::c_void,
    ) -> std::ffi::c_int;
}

fn register_vector_extension(conn: &Connection) -> anyhow::Result<()> {
    let mut error = std::ptr::null_mut();
    // The extension is compiled with SQLITE_CORE and registers only on this connection.
    let result = unsafe { sqlite3_vector_init(conn.handle(), &mut error, std::ptr::null()) };
    if result != rusqlite::ffi::SQLITE_OK {
        let message = if error.is_null() {
            format!("SQLite error {result}")
        } else {
            let message = unsafe { std::ffi::CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { rusqlite::ffi::sqlite3_free(error.cast()) };
            message
        };
        anyhow::bail!("cannot initialize SQLite-Vector: {message}");
    }
    Ok(())
}
