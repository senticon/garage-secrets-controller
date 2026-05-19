use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "garage-secrets-controller",
    about = "A controller for provisioning Garage S3 buckets, access keys, and grants from KMS/secret backends."
)]
pub struct Config {
    #[arg(long, env = "BAO_ADDR", default_value = "http://openbao:8200")]
    pub bao_addr: String,

    #[arg(long, env = "BAO_TOKEN", default_value = "")]
    pub bao_token: String,

    #[arg(long, env = "BAO_KV_MOUNT", default_value = "kv")]
    pub bao_kv_mount: String,

    #[arg(long, env = "BAO_PREFIX", default_value = "garage")]
    pub bao_prefix: String,

    #[arg(long, env = "GARAGE_ADMIN_URL", default_value = "http://garage:3903")]
    pub garage_admin_url: String,

    #[arg(long, env = "GARAGE_ADMIN_TOKEN", default_value = "")]
    pub garage_admin_token: String,

    #[arg(long, env = "POLL_INTERVAL_SECONDS", default_value_t = 30)]
    pub poll_interval_seconds: u64,

    #[arg(long, env = "ONCE", default_value_t = false)]
    pub once: bool,

    #[arg(long, env = "DRY_RUN", default_value_t = false)]
    pub dry_run: bool,
}
