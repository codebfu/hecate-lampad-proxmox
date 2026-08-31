//! Copyright (C) 2026 Gaultier HUBERT
//! SPDX-License-Identifier: GPL-3.0-or-later

//! Linux-focused Proxmox VM console helper.

#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

mod backend;
mod server;
mod session;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "hecate-lampad-proxmox",
    about = "Hecate Proxmox console helper",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Serve local IPC for the hecate-lampad agent.
    Run {
        /// Override the IPC socket path.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Print helper capabilities as JSON.
    Info,
}

pub fn default_socket_path() -> PathBuf {
    PathBuf::from("/run/hecate-lampad/proxmox.sock")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match Cli::parse().command {
        Commands::Run { socket } => server::run(socket.unwrap_or_else(default_socket_path)).await,
        Commands::Info => {
            println!("{}", serde_json::to_string_pretty(&backend::helper_info())?);
            Ok(())
        }
    }
}
