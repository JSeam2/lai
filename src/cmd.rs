use crate::{
    cli::{Cli, Command, ConfigCommand},
    download, key, model, pet, serve,
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            detached,
            pet,
            port,
            ready_signal,
        } => serve::run(detached, pet, port, ready_signal),
        Command::Kill => serve::kill(),
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
        } => model::run(action),
        Command::Config {
            command: ConfigCommand::Pet { command },
        } => pet::config::run(command),
    }
}
