use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::cli::{PetAnimationAction, PetCommand, PetModelAction};

#[derive(Debug, Default)]
pub struct PetConfig {
    pub model: Option<Vec<u8>>,
    model_path: Option<String>,
    pub animations: Animations,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Animations {
    pub idle: String,
    pub waiting: String,
    pub reply: String,
    pub error: String,
    pub shutdown: String,
}

impl Default for Animations {
    fn default() -> Self {
        Self {
            idle: "1_normal".into(),
            waiting: "6_curious".into(),
            reply: "2_happy".into(),
            error: "3_sad".into(),
            shutdown: "3_sad".into(),
        }
    }
}

fn config_path() -> anyhow::Result<PathBuf> {
    Ok(lai::root()?.join("config.toml"))
}

fn document(path: &Path) -> anyhow::Result<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(text) => text
            .parse::<DocumentMut>()
            .with_context(|| format!("{} is not valid TOML", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

pub fn load() -> anyhow::Result<PetConfig> {
    let path = config_path()?;
    parse(&document(&path)?, &path)
        .with_context(|| format!("invalid pet configuration in {}", path.display()))
}

fn read_model(model: &str, config_path: &Path) -> anyhow::Result<Vec<u8>> {
    ensure!(!model.trim().is_empty(), "model path must not be empty");
    let model_path = config_path.parent().unwrap_or(Path::new(".")).join(model);
    let bytes = fs::read(&model_path)
        .with_context(|| format!("cannot read pet model {}", model_path.display()))?;
    ensure!(
        bytes.len() >= 12
            && &bytes[..4] == b"glTF"
            && bytes[4..8] == [2, 0, 0, 0]
            && u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize == bytes.len(),
        "{} is not a valid GLB version 2 file",
        model_path.display()
    );
    Ok(bytes)
}

fn parse(document: &DocumentMut, path: &Path) -> anyhow::Result<PetConfig> {
    let mut config = PetConfig::default();
    let Some(pet) = document.get("pet") else {
        return Ok(config);
    };
    let pet = pet.as_table_like().context("pet must be a table")?;
    for (key, _) in pet.iter() {
        ensure!(
            matches!(key, "model" | "animations"),
            "unknown pet setting '{key}'"
        );
    }
    if let Some(model) = pet.get("model") {
        let model = model
            .as_str()
            .context("pet model must be a GLB file path")?;
        config.model = Some(read_model(model, path)?);
        config.model_path = Some(model.to_owned());
    }
    if let Some(animations) = pet.get("animations") {
        let animations = animations
            .as_table_like()
            .context("pet animations must be a table")?;
        for (name, clip) in animations.iter() {
            let target = match name {
                "idle" => &mut config.animations.idle,
                "waiting" => &mut config.animations.waiting,
                "reply" => &mut config.animations.reply,
                "error" => &mut config.animations.error,
                "shutdown" => &mut config.animations.shutdown,
                _ => anyhow::bail!("unknown animation event '{name}'"),
            };
            let clip = clip
                .as_str()
                .with_context(|| format!("animation '{name}' must be a string"))?;
            ensure!(
                !clip.trim().is_empty(),
                "animation '{name}' must not be empty"
            );
            *target = clip.to_owned();
        }
    }
    Ok(config)
}

pub fn run(command: PetCommand) -> anyhow::Result<()> {
    if matches!(command, PetCommand::Show) {
        let config = load()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "model": config.model_path,
                "animations": config.animations,
            }))?
        );
        return Ok(());
    }
    let path = config_path()?;
    let directory = path.parent().context("cannot locate LAI directory")?;
    let mut document = document(&path)?;

    // Reject malformed tables rather than replacing existing configuration.
    if let Some(pet) = document.get("pet") {
        let pet = pet
            .as_table_like()
            .context("pet must be a table in config.toml")?;
        if let Some(animations) = pet.get("animations") {
            ensure!(
                animations.as_table_like().is_some(),
                "pet animations must be a table in config.toml"
            );
        }
    }

    match command {
        PetCommand::Model {
            action: PetModelAction::Set { path: model },
        } => {
            read_model(&model, &path)?;
            let pet = pet_table(&mut document)?;
            pet.insert("model", value(model));
        }
        PetCommand::Model {
            action: PetModelAction::Unset,
        } => {
            let pet = existing_pet_table(&mut document)?;
            ensure!(pet.remove("model").is_some(), "no pet model is configured");
        }
        PetCommand::Animation {
            action: PetAnimationAction::Set { event, clip },
        } => {
            ensure!(!clip.trim().is_empty(), "animation clip must not be empty");
            let pet = pet_table(&mut document)?;
            if !pet.contains_key("animations") {
                pet.insert("animations", Item::Table(Table::new()));
            }
            pet.get_mut("animations")
                .unwrap()
                .as_table_like_mut()
                .unwrap()
                .insert(event.name(), value(clip));
        }
        PetCommand::Animation {
            action: PetAnimationAction::Unset { event },
        } => {
            let pet = existing_pet_table(&mut document)?;
            let animations = pet
                .get_mut("animations")
                .and_then(Item::as_table_like_mut)
                .context("no pet animations are configured")?;
            ensure!(
                animations.remove(event.name()).is_some(),
                "no animation is configured for {}",
                event.name()
            );
        }
        PetCommand::Show => unreachable!(),
    }
    crate::model::save(directory, &path, &document)?;
    Ok(())
}

fn pet_table(document: &mut DocumentMut) -> anyhow::Result<&mut dyn toml_edit::TableLike> {
    if !document.contains_key("pet") {
        document.insert("pet", Item::Table(Table::new()));
    }
    existing_pet_table(document)
}

fn existing_pet_table(document: &mut DocumentMut) -> anyhow::Result<&mut dyn toml_edit::TableLike> {
    document
        .get_mut("pet")
        .and_then(Item::as_table_like_mut)
        .context("no pet configuration exists or pet is not a table")
}
