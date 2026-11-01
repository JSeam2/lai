use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(version, about = "Local AI for Apple Silicon")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Serve the configured model through an OpenAI-compatible API
    Serve {
        /// Run the server in the background
        #[arg(short, long)]
        detached: bool,
        /// Local port to listen on
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, hide = true)]
        ready_signal: bool,
    },
    /// Download a model repository from Hugging Face
    Download {
        /// Hugging Face model repository in the form owner/model
        model: String,
    },
    /// Manage credentials in macOS Keychain
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Manage LAI configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Manage the selected model
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
}

#[derive(Subcommand)]
pub enum ModelAction {
    /// Set the model when none is configured
    Set {
        /// Hugging Face model repository in the form owner/model
        model: String,
    },
    /// Remove the selected model
    Unset,
    /// Replace the selected model
    Update {
        /// Hugging Face model repository in the form owner/model
        model: String,
    },
}

#[derive(Subcommand)]
pub enum KeyAction {
    /// Store a new credential
    Set {
        /// Credential provider
        provider: Provider,
    },
    /// Remove a stored credential
    Unset {
        /// Credential provider
        provider: Provider,
    },
    /// Replace an existing credential
    Rotate {
        /// Credential provider
        provider: Provider,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Provider {
    Huggingface,
}
