use lai::memory::{MemoryStore, NewMemory};

#[test]
fn memories_survive_restart_and_preserve_history() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("memory.sqlite");
    let mut store = MemoryStore::open(&path)?;
    let old = store.insert(&NewMemory::new("User lives in Paris", "I live in Paris"))?;
    let mut updated = NewMemory::new("User lives in Berlin", "I moved to Berlin");
    updated.supersedes = Some(old);
    let new = store.insert(&updated)?;
    drop(store);
    let store = MemoryStore::open(&path)?;
    assert_eq!(store.get(new)?.unwrap().supersedes, Some(old));
    assert_eq!(store.get(old)?.unwrap().text, "User lives in Paris");
    assert_eq!(store.get(new)?.unwrap().source, "I moved to Berlin");
    Ok(())
}

#[test]
fn pending_jobs_are_deduplicated_and_recoverable() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("memory.sqlite");
    let store = MemoryStore::open(&path)?;
    store.enqueue("I like tea")?;
    store.enqueue("I like tea")?;
    drop(store);
    let mut store = MemoryStore::open(&path)?;
    let job = store.next_job()?.unwrap();
    store.finish_job(job.id, &[NewMemory::new("User likes tea", &job.text)])?;
    assert!(store.next_job()?.is_none());
    store.enqueue("I like tea")?;
    assert!(store.next_job()?.is_none());
    assert_eq!(store.list()?.len(), 1);
    Ok(())
}

#[test]
fn delete_clear_and_separate_profiles() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut a = MemoryStore::open(&dir.path().join("a.sqlite"))?;
    let b = MemoryStore::open(&dir.path().join("b.sqlite"))?;
    let id = a.insert(&NewMemory::new("User likes tea", "I like tea"))?;
    assert!(b.list()?.is_empty());
    a.delete(id)?;
    assert!(a.list()?.is_empty());
    a.enqueue("hello")?;
    a.clear()?;
    assert!(a.next_job()?.is_none());
    Ok(())
}

#[test]
fn extraction_requires_source_evidence_and_rejects_invalid_output() -> anyhow::Result<()> {
    use lai::memory::parse_extraction;
    let source = "I moved to Berlin and like tea.";
    let memories = parse_extraction(r#"[{"text":"User likes tea","source":"like tea"}]"#, source)?;
    assert_eq!(memories.len(), 1);
    assert!(
        parse_extraction(
            r#"[{"text":"User owns a yacht","source":"owns a yacht"}]"#,
            source
        )
        .is_err()
    );
    assert!(parse_extraction("not json", source).is_err());
    Ok(())
}

#[test]
fn vector_search_ranks_semantic_embeddings_and_survives_restart() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vectors.sqlite");
    let mut store = MemoryStore::open(&path)?;
    store.configure_vectors("embedding-v1", 3)?;
    assert!(!store.has_embeddings()?);
    let a = store.insert(&NewMemory::new("User enjoys cycling", "I like cycling"))?;
    let b = store.insert(&NewMemory::new("User likes tea", "I like tea"))?;
    store.put_embedding(a, &[1.0, 0.0, 0.0])?;
    store.put_embedding(b, &[0.0, 1.0, 0.0])?;
    assert!(store.has_embeddings()?);
    drop(store);
    let mut store = MemoryStore::open(&path)?;
    store.configure_vectors("embedding-v1", 3)?;
    let matches = store.search_vectors(&[0.9, 0.1, 0.0], 2)?;
    assert_eq!(matches.iter().map(|m| m.id).collect::<Vec<_>>(), vec![a, b]);
    store.delete(a)?;
    assert_eq!(store.search_vectors(&[1.0, 0.0, 0.0], 2)?.len(), 1);
    Ok(())
}

#[test]
fn vector_model_change_marks_old_memories_for_reembedding() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut store = MemoryStore::open(&dir.path().join("vectors.sqlite"))?;
    store.configure_vectors("embedding-v1", 3)?;
    let id = store.insert(&NewMemory::new("User enjoys cycling", "I like cycling"))?;
    store.put_embedding(id, &[1.0, 0.0, 0.0])?;
    assert!(store.unembedded(10)?.is_empty());
    store.configure_vectors("embedding-v2", 3)?;
    assert_eq!(store.unembedded(10)?[0].id, id);
    assert!(store.search_vectors(&[1.0, 0.0, 0.0], 2)?.is_empty());
    assert!(store.put_embedding(id, &[0.0, 0.0, 0.0]).is_err());
    assert!(store.put_embedding(id, &[f32::NAN, 0.0, 1.0]).is_err());
    assert!(store.put_embedding(id, &[1.0, 0.0]).is_err());
    Ok(())
}

#[test]
fn upgrades_full_text_database_without_losing_memories() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("old.sqlite");
    {
        let conn = rusqlite::Connection::open(&path)?;
        conn.execute_batch("CREATE TABLE memories (id INTEGER PRIMARY KEY, text TEXT NOT NULL, source TEXT NOT NULL, created_at TEXT NOT NULL, supersedes INTEGER REFERENCES memories(id), UNIQUE(text, source));
            CREATE VIRTUAL TABLE memories_fts USING fts5(text, source, content='memories', content_rowid='id');
            CREATE TABLE jobs (id INTEGER PRIMARY KEY, digest BLOB NOT NULL UNIQUE, text TEXT NOT NULL);
            CREATE TABLE processed (digest BLOB PRIMARY KEY);
            INSERT INTO memories VALUES (1, 'User likes cycling', 'I like cycling', '2026-01-01', NULL);
            PRAGMA user_version = 1;")?;
    }
    let mut store = MemoryStore::open(&path)?;
    store.configure_vectors("model", 3)?;
    assert_eq!(store.unembedded(10)?[0].id, 1);
    drop(store);
    let conn = rusqlite::Connection::open(&path)?;
    let fts_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='memories_fts')",
        [],
        |r| r.get(0),
    )?;
    assert!(!fts_exists);
    Ok(())
}

#[test]
fn stale_connections_cannot_mix_embedding_models() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("shared.sqlite");
    let mut first = MemoryStore::open(&path)?;
    let mut second = MemoryStore::open(&path)?;
    first.configure_vectors("v1", 3)?;
    let id = first.insert(&NewMemory::new("User likes tea", "I like tea"))?;
    second.configure_vectors("v2", 3)?;
    assert!(first.put_embedding(id, &[1.0, 0.0, 0.0]).is_err());
    assert!(first.search_vectors(&[1.0, 0.0, 0.0], 2).is_err());
    assert!(second.unembedded(10)?.iter().any(|memory| memory.id == id));
    Ok(())
}

#[test]
fn changing_vector_dimension_reindexes_after_reopen() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("dimensions.sqlite");
    let mut old = MemoryStore::open(&path)?;
    old.configure_vectors("old", 3)?;
    let id = old.insert(&NewMemory::new("User likes tea", "I like tea"))?;
    old.put_embedding(id, &[1.0, 0.0, 0.0])?;
    drop(old);
    let mut new = MemoryStore::open(&path)?;
    new.configure_vectors("new", 2)?;
    assert_eq!(new.unembedded(1)?[0].id, id);
    new.put_embedding(id, &[1.0, 0.0])?;
    assert_eq!(new.search_vectors(&[1.0, 0.0], 1)?[0].id, id);
    Ok(())
}
