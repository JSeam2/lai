use crate::{
    cli::{Cli, Command, ConfigCommand},
    download, key, memory_control, model, pet, server,
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            detached,
            pet,
            port,
            ready_signal,
        } => server::run(detached, pet, port, ready_signal),
        Command::Kill => server::kill(),
        Command::Memory { action } => memory_control::run(action),
        Command::Config {
            command: ConfigCommand::Memory { action },
        } => memory_control::configure(action),
        Command::Download { model } => download::download(&model),
        Command::Key { action } => key::run(action),
        Command::Config {
            command:
                ConfigCommand::SpeculativeDecoding {
                    enabled,
                    draft_model,
                },
        } => model::set_speculative(enabled, draft_model),
        Command::Config {
            command: ConfigCommand::SystemPrompt { prompt },
        } => model::set_system_prompt(&prompt),
        Command::Config {
            command: ConfigCommand::Model { action },
        } => model::run(action, model::ModelKind::Chat),
        Command::Config {
            command: ConfigCommand::ModelMemory { action },
        } => model::run(action, model::ModelKind::Memory),
        Command::Config {
            command: ConfigCommand::Pet { command },
        } => pet::config::run(command),
    }
}
