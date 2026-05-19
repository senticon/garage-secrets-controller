#![forbid(unsafe_code)]

mod config;
mod error;
mod garage;
mod key_storage;
mod model;
mod openbao;
mod reconciler;

use std::time::Duration;

use clap::Parser;
use config::Config;
use error::{AppError, Result};
use reconciler::Reconciler;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cfg = Config::parse();
    validate_config(&cfg)?;

    let reconciler = Reconciler {
        key_store: openbao::OpenBaoClient::new(
            cfg.bao_addr.clone(),
            cfg.bao_kv_mount.clone(),
            cfg.bao_token.clone(),
        ),
        garage: garage::GarageClient::new(
            cfg.garage_admin_url.clone(),
            cfg.garage_admin_token.clone(),
        ),
        prefix: cfg.bao_prefix.clone(),
        dry_run: cfg.dry_run,
    };

    if cfg.once {
        reconciler.reconcile_once().await?;
        info!("single reconciliation pass completed");
        return Ok(());
    }

    let interval = Duration::from_secs(cfg.poll_interval_seconds);
    loop {
        if let Err(err) = reconciler.reconcile_once().await {
            error!(error = %err, "reconciliation loop failed");
        }
        tokio::time::sleep(interval).await;
    }
}

fn validate_config(cfg: &Config) -> Result<()> {
    if cfg.bao_token.is_empty() {
        return Err(AppError::MissingConfig("BAO_TOKEN/--bao-token"));
    }
    if cfg.garage_admin_token.is_empty() {
        return Err(AppError::MissingConfig(
            "GARAGE_ADMIN_TOKEN/--garage-admin-token",
        ));
    }
    Ok(())
}
