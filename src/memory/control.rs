use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use crate::{
    cli::{MemoryAction, MemoryConfigAction},
    model,
};
use anyhow::{Context, ensure};
use lai::memory::MemoryStore;
use toml_edit::{DocumentMut, value};

fn config(root: &Path) -> anyhow::Result<DocumentMut> {
    let path = root.join("config.toml");
    match fs::read_to_string(&path) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("{} is not valid TOML", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(e.into()),
    }
}
fn validate_profile(name: &str) -> anyhow::Result<()> {
    let mut components = Path::new(name).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
            && name != "."
            && name != ".."
            && name.ends_with(".sqlite"),
        "memory profile must be a SQLite filename ending in .sqlite"
    );
    Ok(())
}
pub fn configure(action: MemoryConfigAction) -> anyhow::Result<()> {
    let root = lai::root()?;
    let mut config = config(&root)?;
    match action {
        MemoryConfigAction::Enabled { enabled } => config["memory_enabled"] = value(enabled),
        MemoryConfigAction::Profile { filename } => {
            validate_profile(&filename)?;
            config["memory_profile"] = value(filename);
        }
    }
    model::save(&root, &root.join("config.toml"), &config)
}
pub fn settings() -> anyhow::Result<(bool, PathBuf)> {
    let root = lai::root()?;
    let config = config(&root)?;
    let enabled = config
        .get("memory_enabled")
        .map(|v| v.as_bool().context("memory_enabled must be a boolean"))
        .transpose()?
        .unwrap_or(true);
    let filename = config
        .get("memory_profile")
        .map(|v| v.as_str().context("memory_profile must be a string"))
        .transpose()?
        .unwrap_or("default.sqlite");
    validate_profile(filename)?;
    Ok((enabled, root.join("memories").join(filename)))
}
pub fn run(action: MemoryAction) -> anyhow::Result<()> {
    let (_, path) = settings()?;
    let mut store = MemoryStore::open(&path)?;
    match action {
        MemoryAction::List => {
            for memory in store.list()? {
                println!("{}\t{}\t{}", memory.id, memory.created_at, memory.text);
            }
        }
        MemoryAction::Show { id } => {
            let memory = store.get(id)?.context("memory not found")?;
            println!(
                "{}\nDate: {}\nSource: {}",
                memory.text, memory.created_at, memory.source
            );
            if let Some(id) = memory.supersedes {
                println!("Supersedes: {id}");
            }
        }
        MemoryAction::Delete { id } => {
            ensure!(store.get(id)?.is_some(), "memory not found");
            store.delete(id)?;
        }
        MemoryAction::Clear => store.clear()?,
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn open_embedding_model() -> anyhow::Result<(lai::memory::embedding::EmbeddingModel, String)> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let root = lai::root()?;
    let config = config(&root)?;
    let id = config
        .get("model_memory")
        .map(|v| v.as_str().context("model_memory must be a model ID"))
        .transpose()?
        .unwrap_or("mlx-community/embeddinggemma-2-5bit");
    crate::download::validate_model_id(id)?;
    let path = root.join("models").join(id);
    ensure!(
        path.is_dir(),
        "memory model {id} is not installed; run `lai download {id}`"
    );
    let python = std::env::var_os("LAI_MEMORY_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let managed = root.join("embedding-venv/bin/python3");
            if managed.is_file() {
                managed
            } else {
                PathBuf::from("python3")
            }
        });
    log::info!("Loading local memory embedding model {id}");
    let embedder = lai::memory::embedding::EmbeddingModel::start(&python, &path)?;
    let mut files = fs::read_dir(&path)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|file| {
        file.extension().is_some_and(|ext| ext == "safetensors")
            || file.file_name().is_some_and(|name| {
                name == "config.json" || name == "tokenizer.json" || name == "tokenizer_config.json"
            })
    });
    files.sort();
    let mut digest = Sha256::new();
    digest.update(b"embeddinggemma2-retrieval-v1");
    for file in files {
        digest.update(
            file.file_name()
                .context("invalid model file")?
                .as_encoded_bytes(),
        );
        let mut input = fs::File::open(file)?;
        let mut buffer = [0u8; 65536];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
    }
    let hash: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let identity = format!("{id}:{hash}");
    Ok((embedder, identity))
}
