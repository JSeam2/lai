use crate::{
    cli::{Cli, Command, ConfigCommand},
    download, key, model, serve,
};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            detached,
            port,
            ready_signal,
        } => serve::run(detached, port, ready_signal),
        Command::Download { model } => download::download(&model),
        Command::Key { action } => key::run(action),
        Command::Config {
            command: ConfigCommand::Model { action },
        } => model::run(action),
    }
}
