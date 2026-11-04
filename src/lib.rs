pub mod pet {
    pub mod assets;
    pub mod lifecycle;
}

pub mod memory;

/// Return the directory containing LAI's local configuration and data.
pub fn root() -> anyhow::Result<std::path::PathBuf> {
    use anyhow::Context;

    Ok(std::path::PathBuf::from(
        std::env::var_os("HOME").context("HOME is not set; set it to your home directory")?,
    )
    .join(".lai"))
}
