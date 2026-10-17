use anyhow::{ensure, Result};
use clap::{Parser, Subcommand};
use soapbox::{
    api::{self, App},
    config::Config,
    store::Store,
    worker::{self, ApiClient, WorkerConfig},
};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Opt-in isolated compute pools with test credits or Base Sepolia USDC escrow."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    #[command(hide = true)]
    SandboxRun,
    Inspect {
        #[arg(long)]
        config: PathBuf,
    },
    Serve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value = "data/soapbox.sqlite")]
        db: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: SocketAddr,
        #[arg(long)]
        allow_remote: bool,
    },
    Worker {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        coordinator: String,
        #[arg(long, default_value = "SOAPBOX_WORKER_TOKEN")]
        token_env: String,
        #[arg(long)]
        once: bool,
    },
    Request {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        coordinator: String,
        #[arg(long, default_value = "SOAPBOX_CLIENT_TOKEN")]
        token_env: String,
        #[arg(long, default_value = "GET")]
        method: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::SandboxRun => soapbox::sandbox::runner().await?,
        Command::Inspect { config } => {
            let config = WorkerConfig::read(&config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&worker::inventory(&config).await?)?
            );
        }
        Command::Serve {
            config,
            db,
            bind,
            allow_remote,
        } => {
            ensure!(
                bind.ip().is_loopback() || allow_remote,
                "remote bind requires --allow-remote and a trusted TLS reverse proxy"
            );
            let config = Config::read(&config)?;
            if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let store = Store::open(
                &db,
                config.collective_daily_items,
                config.artifact_quota_bytes,
            )?;
            let settlement = if config.payments.is_some() {
                "USDC_ESCROW"
            } else {
                "TEST_CREDIT"
            };
            let app = App::new(store, config)?;
            let sweep = app.store.clone();
            let sweeper = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    if let Ok(mut store) = sweep.lock() {
                        if let Err(error) = store.sweep(soapbox::model::now()) {
                            eprintln!("maintenance failed: {error}");
                        }
                    }
                }
            });
            let listener = tokio::net::TcpListener::bind(bind).await?;
            eprintln!(
                "soapbox API v1 listening on {}; settlement={settlement}",
                listener.local_addr()?
            );
            axum::serve(listener, api::router(app))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            sweeper.abort();
        }
        Command::Worker {
            config,
            coordinator,
            token_env,
            once,
        } => {
            let token = std::env::var(token_env)?;
            let config = WorkerConfig::read(&config)?;
            let client = ApiClient::new(&coordinator, token)?;
            if once {
                worker::run(client, config, true).await?;
            } else {
                loop {
                    match worker::run(client.clone(), config.clone(), false).await {
                        Ok(()) => break,
                        Err(error) => eprintln!("worker reconnecting after error: {error}"),
                    }
                    tokio::select! {
                        _=tokio::signal::ctrl_c()=>break,
                        _=tokio::time::sleep(std::time::Duration::from_secs(5))=>{},
                    }
                }
            }
        }
        Command::Request {
            coordinator,
            token_env,
            method,
            path,
            file,
        } => {
            ensure!(
                path.starts_with("/v1/") && !path.contains(".."),
                "request path must start with /v1/"
            );
            let client = ApiClient::new(&coordinator, std::env::var(token_env)?)?;
            let body = file
                .map(|f| -> Result<serde_json::Value> {
                    Ok(serde_json::from_slice(&std::fs::read(f)?)?)
                })
                .transpose()?;
            let result: serde_json::Value = client.request(method.parse()?, &path, body).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}
