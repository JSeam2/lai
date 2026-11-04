use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(version, about = "Local AI for Apple Silicon")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Stop the detached LAI server
    Kill,
    /// Serve the configured model through an OpenAI-compatible API
    Serve {
        /// Run the server in the background
        #[arg(short, long)]
        detached: bool,
        /// Show the desktop pet while serving
        #[arg(short, long)]
        pet: bool,
        /// Local port to listen on
        #[arg(long, default_value_t = 8989)]
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
    /// Inspect and manage saved memories
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Manage LAI configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Enable or disable speculative decoding
    #[command(name = "speculativeDecoding")]
    SpeculativeDecoding {
        /// Whether speculative decoding is enabled: true or false
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
        /// Hugging Face ID of the draft model (defaults to the Gemma 4 E4B assistant)
        #[arg(long)]
        draft_model: Option<String>,
    },
    /// Set the default system prompt (pass an empty string to clear it)
    #[command(name = "systemPrompt")]
    SystemPrompt { prompt: String },
    /// Manage the selected model
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Manage the memory embedding model
    #[command(name = "modelMemory")]
    ModelMemory {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// Configure persistent memory
    Memory {
        #[command(subcommand)]
        action: MemoryConfigAction,
    },
    /// Configure the desktop pet
    Pet {
        #[command(subcommand)]
        command: PetCommand,
    },
}

#[derive(Subcommand)]
pub enum PetCommand {
    /// Show the effective pet settings
    Show,
    /// Set or reset the pet's GLB model
    Model {
        #[command(subcommand)]
        action: PetModelAction,
    },
    /// Set or reset the animation for an event
    Animation {
        #[command(subcommand)]
        action: PetAnimationAction,
    },
}

#[derive(Subcommand)]
pub enum PetModelAction {
    /// Use a GLB model (relative paths are resolved from ~/.lai)
    Set { path: String },
    /// Use the bundled cat
    Unset,
}

#[derive(Subcommand)]
pub enum PetAnimationAction {
    /// Assign a GLB animation clip to an event
    Set { event: PetEvent, clip: String },
    /// Restore the default animation for an event
    Unset { event: PetEvent },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PetEvent {
    Idle,
    Waiting,
    Reply,
    Error,
    Shutdown,
}

impl PetEvent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Waiting => "waiting",
            Self::Reply => "reply",
            Self::Error => "error",
            Self::Shutdown => "shutdown",
        }
    }
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

#[derive(Subcommand)]
pub enum MemoryAction {
    /// List saved memories
    List,
    /// Show a memory and its source
    Show { id: i64 },
    /// Delete a saved memory
    Delete { id: i64 },
    /// Clear saved memories and queued jobs
    Clear,
}

#[derive(Subcommand)]
pub enum MemoryConfigAction {
    /// Enable or disable persistent memory
    Enabled {
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    /// Select a SQLite filename under ~/.lai/memories
    Profile { filename: String },
}
