// src/main.rs

use clap::Parser;
use env_logger::Builder;
use eyre::{eyre, Result};
use log::{debug, error, info};
use native_tls::TlsConnector;
use std::fs::OpenOptions;
use std::io::Write;

mod cfg;
mod cli;
mod client_ops;
mod imap_filter;
mod message;
mod oauth2;
mod thread;
mod utils;

use cfg::config::load_config;
use cli::Cli;
use imap_filter::IMAPFilter;
use oauth2::{OAuth2Credentials, XOAuth2Authenticator};

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
    let config = load_config(&cli.config)?;

    // 2) Resolve connection parameters, preferring CLI/env over config file
    let imap_domain = cli.imap_domain.or(config.imap_domain.clone()).ok_or_else(|| {
        error!("IMAP domain is required but missing.");
        eyre!("IMAP domain is required")
    })?;

    let imap_username = cli.imap_username.or(config.imap_username.clone()).ok_or_else(|| {
        error!("IMAP username is required but missing.");
        eyre!("IMAP username is required")
    })?;

    // Resolve OAuth2 credentials (CLI/env takes precedence over config)
    let oauth2_client_id = cli.oauth2_client_id.or(config.oauth2_client_id.clone());
    let oauth2_client_secret = cli.oauth2_client_secret.or(config.oauth2_client_secret.clone());
    let oauth2_refresh_token = cli.oauth2_refresh_token.or(config.oauth2_refresh_token.clone());

    // Check if we have OAuth2 credentials
    let use_oauth2 = oauth2_client_id.is_some() && oauth2_client_secret.is_some() && oauth2_refresh_token.is_some();

    debug!("Using IMAP server: {}  user: {}", imap_domain, imap_username);

    // 3) Connect & authenticate
    let tls = TlsConnector::builder().build()?;
    let client_conn = imap::connect((imap_domain.as_str(), 993), imap_domain.as_str(), &tls)
        .map_err(|e| eyre!("Failed to connect to {}: {}", imap_domain, e))?;

    let mut client = if use_oauth2 {
        // OAuth2 authentication
        info!("Using OAuth2 authentication");
        let creds = OAuth2Credentials {
            client_id: oauth2_client_id.unwrap().unsecure().to_string(),
            client_secret: oauth2_client_secret.unwrap().unsecure().to_string(),
            refresh_token: oauth2_refresh_token.unwrap().unsecure().to_string(),
        };

        let access_token = creds.refresh_access_token()?;
        let authenticator = XOAuth2Authenticator::new(&imap_username, &access_token);

        client_conn
            .authenticate("XOAUTH2", &authenticator)
            .map_err(|(e, _)| eyre!("OAuth2 IMAP authentication failed: {}", e))?
    } else {
        // Password authentication
        info!("Using password authentication");
        let imap_password = cli.imap_password.or(config.imap_password.clone()).ok_or_else(|| {
            error!("IMAP password is required but missing (no OAuth2 credentials provided either).");
            eyre!("IMAP password or OAuth2 credentials required")
        })?;

        client_conn
            .login(&imap_username, imap_password.unsecure())
            .map_err(|(e, _)| eyre!("IMAP login failed: {}", e))?
    };

    info!("✅ Connected and logged in");

    client.debug = cli.debug;
    debug!("Low‐level IMAP protocol debug enabled on client");

    // 4) Run the filter — pass the entire `config` along with the logged‐in client
    let mut filter = IMAPFilter::new(client, config);
    filter.execute()?;

    info!("✅ IMAP Filter execution completed");
    Ok(())
}
