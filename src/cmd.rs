use crate::{
    cli::{Cli, Command, ConfigCommand, InferenceCommand},
    download, key, model, serve,
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            detached,
            port,
            ready_signal,
        } => serve::run(detached, port, ready_signal),
        Command::Kill => serve::kill(),
        Command::Download { model } => download::download(&model),
        Command::Key { action } => key::run(action),
        Command::Model {
            command:
                InferenceCommand::SpeculativeDecoding {
                    enabled,
                    draft_model,
                },
        } => model::set_speculative(enabled, draft_model),
        Command::Config {
            command: ConfigCommand::Model { action },
        } => model::run(action),
    }
}
