use std::{
    env, fs, io,
    path::{Component, Path},
    time::Duration,
};

use anyhow::{Context, ensure};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{
    blocking::Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct ModelInfo {
    sha: String,
    #[serde(default)]
    tags: Vec<String>,
    siblings: Vec<ModelFile>,
}

#[derive(Deserialize)]
struct ModelFile {
    rfilename: String,
}

pub fn download(model: &str) -> anyhow::Result<()> {
    validate_model_id(model)?;
    let parts: Vec<_> = model.split('/').collect();
    let destination = lai::root()?.join("models").join(parts[0]).join(parts[1]);
    let endpoint = match env::var("HF_ENDPOINT") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => "https://huggingface.co".to_owned(),
        Err(env::VarError::NotUnicode(_)) => anyhow::bail!(
            "HF_ENDPOINT is not valid UTF-8; unset it or set it to a valid Hugging Face URL"
        ),
    };
    let endpoint_url = reqwest::Url::parse(&endpoint).context(
        "HF_ENDPOINT is not a valid URL; unset it to use Hugging Face or set it to an HTTP(S) URL",
    )?;
    ensure!(
        matches!(endpoint_url.scheme(), "http" | "https")
            && endpoint_url.host_str().is_some()
            && endpoint_url.username().is_empty()
            && endpoint_url.password().is_none()
            && endpoint_url.query().is_none()
            && endpoint_url.fragment().is_none(),
        "HF_ENDPOINT must be an HTTP(S) URL without credentials, query, or fragment; unset it to use Hugging Face"
    );
    let endpoint = endpoint.trim_end_matches('/');
    let mut headers = HeaderMap::new();
    if let Some(token) = crate::key::huggingface_token()? {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}")).context("Hugging Face token contains invalid characters; check HF_TOKEN or replace the stored key with `lai key rotate huggingface`")?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }
    let client = Client::builder().default_headers(headers).build()?;
    log::info!("Checking MLX compatibility for {model}...");
    let info: ModelInfo = client
        .get(format!("{endpoint}/api/models/{model}"))
        .send()
        .context("failed to fetch model metadata; check your network connection and HF_ENDPOINT")?
        .error_for_status()
        .context("Hugging Face rejected the model metadata request; check the model ID and token permissions and accept the model license if required")?
        .json()
        .context("invalid model metadata from Hugging Face")?;
    ensure!(
        info.sha.len() == 40 && info.sha.chars().all(|c| c.is_ascii_hexdigit()),
        "invalid model revision returned by Hugging Face"
    );
    ensure!(
        !info.siblings.is_empty(),
        "model repository contains no files"
    );
    if info.tags.iter().any(|tag| tag.eq_ignore_ascii_case("mlx")) {
        log::info!("Model is tagged as MLX compatible.");
    } else {
        log::warn!(
            "this model is not tagged as MLX compatible on Hugging Face; it may not work with MLX."
        );
    }
    // Validate every path before creating files. Repository paths must remain within the destination.
    for file in &info.siblings {
        ensure!(
            safe_file_path(&file.rfilename),
            "invalid file path in model repository: {:?}",
            file.rfilename
        );
    }
    log::info!("Downloading {model} to {}", destination.display());
    let file_count = info.siblings.len();
    for (index, file) in info.siblings.into_iter().enumerate() {
        let target = destination.join(&file.rfilename);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create {}; check the directory permissions and available disk space",
                    parent.display()
                )
            })?;
        }
        let mut url = reqwest::Url::parse(&format!("{endpoint}/{model}/resolve/{}/", info.sha))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid Hugging Face endpoint"))?;
            segments.pop_if_empty();
            segments.extend(file.rfilename.split('/'));
        }
        let response = client
            .get(url)
            .send()
            .with_context(|| format!("failed to download {}", file.rfilename))?
            .error_for_status()
            .with_context(|| format!("failed to download {}", file.rfilename))?;
        let mut output = fs::File::create(&target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        let progress = match response.content_length() {
            Some(size) => {
                let bar = ProgressBar::new(size);
                bar.set_style(ProgressStyle::with_template(
                    "{msg} [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})",
                ).context("failed to configure download progress display")?);
                bar
            }
            None => {
                let bar = ProgressBar::new_spinner();
                bar.set_style(
                    ProgressStyle::with_template("{spinner} {msg} {bytes} ({bytes_per_sec})")
                        .context("failed to configure download progress display")?,
                );
                bar.enable_steady_tick(Duration::from_millis(100));
                bar
            }
        };
        progress.set_message(format!("[{}/{}] {}", index + 1, file_count, file.rfilename));
        let result = io::copy(&mut progress.wrap_read(response), &mut output).with_context(|| {
            format!(
                "failed to save {}; check your connection and available disk space",
                target.display()
            )
        });
        progress.finish_and_clear();
        result?;
    }
    log::info!("Model downloaded to {}", destination.display());
    Ok(())
}

pub fn validate_model_id(model: &str) -> anyhow::Result<()> {
    let parts: Vec<_> = model.split('/').collect();
    ensure!(
        parts.len() == 2 && parts.iter().all(|part| valid_name(part)),
        "model must be a Hugging Face repository in the form owner/model"
    );
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn safe_file_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
