use lai::memory::embedding::{EmbeddingModel, EmbeddingPurpose};

#[test]
fn reports_missing_embedding_runtime() {
    let result = EmbeddingModel::start(
        std::path::Path::new("/definitely/missing/python"),
        std::path::Path::new("/missing/model"),
    );
    assert!(result.is_err());
}

#[test]
#[ignore = "requires the local EmbeddingGemma 2 model, MLX Python environment and Apple GPU"]
fn local_model_returns_semantic_vectors() -> anyhow::Result<()> {
    let python = std::env::var("LAI_MEMORY_PYTHON")?;
    let model = std::env::var("LAI_TEST_EMBEDDING_MODEL")?;
    let mut embedder = EmbeddingModel::start(python.as_ref(), model.as_ref())?;
    let query = embedder.embed("Which planet is red?", EmbeddingPurpose::Query)?;
    let mars = embedder.embed(
        "Mars is known as the red planet.",
        EmbeddingPurpose::Document,
    )?;
    let venus = embedder.embed(
        "Venus is often called Earth's twin.",
        EmbeddingPurpose::Document,
    )?;
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    assert_eq!(query.len(), 768);
    assert!(dot(&query, &mars) > dot(&query, &venus));
    let dir = tempfile::tempdir()?;
    let mut store = lai::memory::MemoryStore::open(&dir.path().join("memory.sqlite"))?;
    store.configure_vectors("embeddinggemma2-test", 768)?;
    let mars_id = store.insert(&lai::memory::NewMemory::new(
        "Mars is known as the red planet.",
        "Mars is the red planet.",
    ))?;
    let venus_id = store.insert(&lai::memory::NewMemory::new(
        "Venus is often called Earth's twin.",
        "Venus is Earth's twin.",
    ))?;
    store.put_embedding(mars_id, &mars)?;
    store.put_embedding(venus_id, &venus)?;
    let results = store.search_vectors(&query, 2)?;
    assert_eq!(
        results.iter().map(|memory| memory.id).collect::<Vec<_>>(),
        vec![mars_id, venus_id]
    );
    let bike_query = embedder.embed(
        "What activity involves pedaling a bicycle?",
        EmbeddingPurpose::Query,
    )?;
    let cycling = embedder.embed("The user enjoys cycling.", EmbeddingPurpose::Document)?;
    let coffee = embedder.embed("The user drinks coffee.", EmbeddingPurpose::Document)?;
    assert!(dot(&bike_query, &cycling) > dot(&bike_query, &coffee));
    Ok(())
}
