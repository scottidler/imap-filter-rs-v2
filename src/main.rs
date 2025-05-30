// src/main.rs

#![allow(dead_code, unused_imports)]

use clap::Parser;
use env_logger::Builder;
use eyre::{Result, eyre};
use log::{debug, info, error};
use native_tls::TlsConnector;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

mod cfg;
mod utils;
mod message;
mod imap_filter;

use cfg::config::load_config;
use imap_filter::IMAPFilter;

#[derive(Parser, Debug)]
#[command(
    name = "imap-filter",
    version,
    about = "IMAP email filtering CLI",
    long_about = None
)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "imap-filter.yml")]
    config: PathBuf,

    /// IMAP server domain
    #[arg(short = 'd', long, env = "IMAP_DOMAIN")]
    imap_domain: Option<String>,

    /// IMAP username
    #[arg(short = 'u', long, env = "IMAP_USERNAME")]
    imap_username: Option<String>,

    /// IMAP password
    #[arg(short = 'p', long, env = "IMAP_PASSWORD")]
    imap_password: Option<String>,
}

fn setup_logging() {
    let log_file = "imap-filter.log";
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .expect("Failed to open log file");

    Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(Box::new(file)))
        .init();
}

fn main() -> Result<()> {
    setup_logging();
    info!("========== Starting IMAP Filter ==========");

    let cli = Cli::parse();
    //debug!("CLI args: {:?}", cli);

    // 1) Load YAML config
    let config = load_config(&cli)?;

    // 2) Resolve connection parameters, preferring CLI/env over config file
    let imap_domain = cli
        .imap_domain
        .or(config.imap_domain.clone())
        .ok_or_else(|| {
            error!("IMAP domain is required but missing.");
            eyre!("IMAP domain is required")
        })?;

    let imap_username = cli
        .imap_username
        .or(config.imap_username.clone())
        .ok_or_else(|| {
            error!("IMAP username is required but missing.");
            eyre!("IMAP username is required")
        })?;

    let imap_password = cli
        .imap_password
        .or(config.imap_password.clone())
        .ok_or_else(|| {
            error!("IMAP password is required but missing.");
            eyre!("IMAP password is required")
        })?;

    debug!(
        "Using IMAP server: {}  user: {}",
        imap_domain, imap_username
    );

    // 3) Connect & authenticate
    let tls = TlsConnector::builder().build()?;
    let mut client = imap::connect((imap_domain.as_str(), 993), imap_domain.as_str(), &tls)
        .map_err(|e| eyre!("Failed to connect to {}: {}", imap_domain, e))?
        .login(&imap_username, &imap_password)
        .map_err(|(e, _)| eyre!("IMAP login failed: {}", e))?;

    info!("✅ Connected and logged in");

    client.debug = true;
    debug!("Low‐level IMAP protocol debug enabled on client");

    // 4) Run the filter — pass the entire `config` along with the logged‐in client
    let mut filter = IMAPFilter::new(client, config);
    filter.execute()?;

    info!("✅ IMAP Filter execution completed");
    Ok(())
}
