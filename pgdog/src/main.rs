//! pgDog, modern PostgreSQL proxy, pooler and query router.

use backend::databases;
use clap::Parser;
use cli::Commands;
use frontend::listener::Listener;
use tokio::runtime::Builder;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use std::{io::IsTerminal, process::exit};

pub mod admin;
pub mod auth;
pub mod backend;
pub mod cli;
pub mod config;
pub mod frontend;
pub mod net;
pub mod plugin;
pub mod state;
pub mod stats;
pub mod util;

/// Setup the logger, so `info!`, `debug!`
/// and other macros actually output something.
///
/// Using try_init and ignoring errors to allow
/// for use in tests (setting up multiple times).
fn logger() {
    let format = fmt::layer()
        .with_ansi(std::io::stderr().is_terminal())
        .with_file(false);
    #[cfg(not(debug_assertions))]
    let format = format.with_target(false);

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let _ = tracing_subscriber::registry()
        .with(format)
        .with(filter)
        .try_init();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Cli::parse();

    logger();

    let mut overrides = config::Overrides::default();

    match args.command {
        Some(Commands::Fingerprint { query, path }) => {
            cli::fingerprint(query, path)?;
            exit(0);
        }

        Some(Commands::Schema) => (),

        Some(Commands::Run {
            pool_size,
            min_pool_size,
            session_mode,
        }) => {
            overrides = config::Overrides {
                min_pool_size,
                session_mode,
                default_pool_size: pool_size,
            };
        }

        None => (),
    }

    info!("🐕 pgDog {}", env!("CARGO_PKG_VERSION"));

    let config = if let Some(database_urls) = args.database_url {
        config::from_urls(&database_urls)?
    } else {
        config::load(&args.config, &args.users)?
    };

    config::overrides(overrides);

    plugin::load_from_config()?;

    let runtime = match config.config.general.workers {
        0 => {
            let mut binding = Builder::new_current_thread();
            binding.enable_all();
            binding
        }
        workers => {
            info!("spawning {} workers", workers);
            let mut builder = Builder::new_multi_thread();
            builder.worker_threads(workers).enable_all();
            builder
        }
    }
    .build()?;

    runtime.block_on(async move { pgdog().await })?;

    Ok(())
}

async fn pgdog() -> Result<(), Box<dyn std::error::Error>> {
    // Preload TLS. Resulting primitives
    // are async, so doing this after Tokio launched seems prudent.
    net::tls::load()?;

    // Load databases and connect if needed.
    databases::init();

    let mut listener = Listener::new("0.0.0.0:6432");
    listener.listen().await?;

    info!("🐕 pgDog is shutting down");

    // Any shutdown routines go below.
    plugin::shutdown();

    Ok(())
}
