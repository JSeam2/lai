mod cli;
mod cmd;
mod download;
mod key;
mod model;
mod serve;
#[cfg(target_os = "macos")]
mod server_control;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("lai=info")).init();
    cmd::run(cli::Cli::parse())
}
