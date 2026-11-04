mod cli;
mod cmd;
mod download;
mod key;
#[path = "memory/control.rs"]
mod memory_control;
mod model;
mod pet;
mod server;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("lai=info")).init();
    cmd::run(cli::Cli::parse())
}
