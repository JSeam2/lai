use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, ensure};
use toml_edit::{DocumentMut, value};

use crate::{
    cli::ModelAction,
    download::{self, validate_model_id},
};

pub fn selected_model() -> anyhow::Result<(String, PathBuf)> {
    let home = env::var_os("HOME")
        .context("HOME is not set; set it to your home directory before starting LAI")?;
    let root = PathBuf::from(home).join(".lai");
    let path = root.join("config.toml");
    let contents = fs::read_to_string(&path).with_context(|| {
        format!(
            "cannot read {}; select a model using `lai config model set <MODEL>`",
            path.display()
        )
    })?;
    let config = contents.parse::<DocumentMut>().with_context(|| {
        format!(
            "{} is not valid TOML; fix the file before starting LAI",
            path.display()
        )
    })?;
    let model = config
        .get("model")
        .and_then(|entry| entry.as_str())
        .context("no valid model is configured; run `lai config model set <MODEL>`")?;
    validate_model_id(model)?;
    let model_path = root.join("models").join(model);
    ensure!(
        model_path.is_dir(),
        "model {model} is not installed at {}; run `lai download {model}`",
        model_path.display()
    );
    Ok((model.to_owned(), model_path))
}

pub fn run(action: ModelAction) -> anyhow::Result<()> {
    let home = env::var_os("HOME")
        .context("HOME is not set; set it to your home directory before configuring a model")?;
    let directory = PathBuf::from(home).join(".lai");
    let path = directory.join("config.toml");
    let mut config = match fs::read_to_string(&path) {
        Ok(contents) => contents.parse::<DocumentMut>().with_context(|| {
            format!(
                "{} is not valid TOML; fix the file before changing the model",
                path.display()
            )
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => DocumentMut::new(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("cannot read {}; check its permissions", path.display()));
        }
    };
    if let Some(existing) = config.get("model") {
        ensure!(
            existing.as_str().is_some(),
            "the model in {} must be a string; fix the configuration before continuing",
            path.display()
        );
    }
    match action {
        ModelAction::Set { model } => {
            validate_model_id(&model)?;
            ensure!(
                !config.contains_key("model"),
                "a model is already configured; use `lai config model update <MODEL>` to change it"
            );
            let model_path = directory.join("models").join(&model);
            match fs::metadata(&model_path) {
                Ok(metadata) => ensure!(
                    metadata.is_dir(),
                    "{} is not a directory; remove or move it before downloading the model",
                    model_path.display()
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    log::warn!(
                        "Model {model} is not installed at {}.",
                        model_path.display()
                    );
                    if prompt_download()? {
                        download::download(&model)?;
                    }
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "cannot check {}; check directory permissions",
                            model_path.display()
                        )
                    });
                }
            }
            config["model"] = value(&model);
            save(&directory, &path, &config)?;
            log::info!("Selected model {model} in {}", path.display());
        }
        ModelAction::Update { model } => {
            validate_model_id(&model)?;
            ensure!(
                config.contains_key("model"),
                "no model is configured; use `lai config model set <MODEL>` first"
            );
            config["model"] = value(&model);
            save(&directory, &path, &config)?;
            log::info!("Updated model to {model} in {}", path.display());
        }
        ModelAction::Unset => {
            ensure!(
                config.contains_key("model"),
                "no model is configured; use `lai config model set <MODEL>` first"
            );
            let comments = config
                .as_table()
                .key("model")
                .and_then(|key| key.leaf_decor().prefix())
                .and_then(|prefix| prefix.as_str())
                .map(str::to_owned);
            config.remove("model");
            if let Some(comments) = comments {
                let trailing = match config.trailing().as_str() {
                    Some(text) => text,
                    None => "",
                };
                config.set_trailing(format!("{comments}{trailing}"));
            }
            save(&directory, &path, &config)?;
            log::info!("Removed the selected model from {}", path.display());
        }
    }
    Ok(())
}

fn prompt_download() -> anyhow::Result<bool> {
    loop {
        eprint!("Download this model now? [y/N] ");
        io::stderr()
            .flush()
            .context("cannot display the download prompt")?;
        let mut answer = String::new();
        let bytes = io::stdin()
            .read_line(&mut answer)
            .context("cannot read the download choice; run this command in a terminal")?;
        ensure!(
            bytes != 0,
            "no response to the download prompt; run this command in a terminal and enter y or n"
        );
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => eprintln!("Please enter y or n."),
        }
    }
}

fn save(
    directory: &std::path::Path,
    path: &std::path::Path,
    config: &DocumentMut,
) -> anyhow::Result<()> {
    fs::create_dir_all(directory).with_context(|| {
        format!(
            "cannot create {}; check directory permissions",
            directory.display()
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).with_context(|| {
        format!(
            "cannot create a temporary config in {}; check directory permissions and disk space",
            directory.display()
        )
    })?;
    temporary
        .write_all(config.to_string().as_bytes())
        .context("cannot write the configuration; check available disk space")?;
    temporary.persist(path).with_context(|| {
        format!(
            "cannot save {}; check directory permissions",
            path.display()
        )
    })?;
    Ok(())
}
