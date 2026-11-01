use anyhow::{Context, ensure};
use std::{env, io::ErrorKind};

use crate::cli::{KeyAction, Provider};

const SERVICE: &str = "lai";
const ACCOUNT: &str = "huggingface";

pub fn run(action: KeyAction) -> anyhow::Result<()> {
    match action {
        KeyAction::Set {
            provider: Provider::Huggingface,
        } => {
            ensure!(
                keychain_get()?.is_none(),
                "A Hugging Face key already exists; use `lai key rotate huggingface` to replace it"
            );
            save_prompted()?;
            log::info!("Hugging Face key saved to macOS Keychain.");
        }
        KeyAction::Rotate {
            provider: Provider::Huggingface,
        } => {
            ensure!(
                keychain_get()?.is_some(),
                "No Hugging Face key is stored; use `lai key set huggingface` first"
            );
            save_prompted()?;
            log::info!("Hugging Face key updated in macOS Keychain.");
        }
        KeyAction::Unset {
            provider: Provider::Huggingface,
        } => {
            keychain_delete()?;
            log::info!("Hugging Face key removed from macOS Keychain.");
        }
    }
    Ok(())
}

fn save_prompted() -> anyhow::Result<()> {
    let token = rpassword::prompt_password("Hugging Face token (input hidden): ").context(
        "failed to read token from terminal; run this command in an interactive terminal",
    )?;
    let token = token.trim();
    ensure!(
        !token.is_empty(),
        "token cannot be empty; enter a valid Hugging Face token"
    );
    keychain_set(token)
}

/// Environment overrides the current directory's .env, which overrides Keychain.
pub fn huggingface_token() -> anyhow::Result<Option<String>> {
    if let Some(token) = env::var_os("HF_TOKEN") {
        let token = token
            .into_string()
            .map_err(|_| anyhow::anyhow!("HF_TOKEN is not valid UTF-8; replace it with a valid Hugging Face token or unset it"))?;
        ensure!(
            !token.trim().is_empty(),
            "HF_TOKEN cannot be empty; set a valid token or unset it"
        );
        return Ok(Some(token));
    }
    match dotenvy::from_filename_iter(".env") {
        Ok(entries) => {
            for entry in entries {
                let (name, value) =
                    entry.context("failed to parse .env; correct its syntax or remove the file")?;
                if name == "HF_TOKEN" {
                    ensure!(
                        !value.trim().is_empty(),
                        "HF_TOKEN in .env cannot be empty; set a valid token or remove the entry"
                    );
                    return Ok(Some(value));
                }
            }
        }
        Err(dotenvy::Error::Io(error)) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to read .env; check file permissions"),
    }
    keychain_get()
}

#[cfg(target_os = "macos")]
fn keychain_get() -> anyhow::Result<Option<String>> {
    match security_framework::passwords::get_generic_password(SERVICE, ACCOUNT) {
        Ok(bytes) => Ok(Some(
            String::from_utf8(bytes).context("stored Hugging Face key is not valid UTF-8")?,
        )),
        Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound
        Err(error) => Err(error).context("failed to read Hugging Face key from macOS Keychain"),
    }
}

#[cfg(target_os = "macos")]
fn keychain_set(token: &str) -> anyhow::Result<()> {
    security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, token.as_bytes())
        .context("failed to save Hugging Face key to macOS Keychain")
}

#[cfg(target_os = "macos")]
fn keychain_delete() -> anyhow::Result<()> {
    security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT)
        .context("failed to remove Hugging Face key from macOS Keychain")
}

#[cfg(not(target_os = "macos"))]
fn keychain_get() -> anyhow::Result<Option<String>> {
    Ok(None)
}
#[cfg(not(target_os = "macos"))]
fn keychain_set(_: &str) -> anyhow::Result<()> {
    anyhow::bail!("Keychain commands require macOS")
}
#[cfg(not(target_os = "macos"))]
fn keychain_delete() -> anyhow::Result<()> {
    anyhow::bail!("Keychain commands require macOS")
}
