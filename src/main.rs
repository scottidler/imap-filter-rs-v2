#![allow(dead_code, unused_imports)]

use clap::Parser;
use env_logger::Builder;
use eyre::{Result, eyre};
use log::{debug, info, error};
use std::path::PathBuf;
use std::io::Write;
use std::fs;
use std::fs::OpenOptions;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// IMAP email filtering CLI
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

mod cfg;
mod utils;
mod message;
mod imap_filter;
// mod uid_tracker;

fn main() -> Result<()> {
    // parse command-line
    let _cli = Cli::parse();

    // initialize logging (if you want it later)
    Builder::new()
        .parse_default_env()
        .try_init()
        .ok();

    // TODO: actually load and run
    // let config = cfg::config::load_config(&_cli)?;
    // let client = /* connect to IMAP server */;
    // let filter = imap_filter::IMAPFilter::new(client, config.message_filter, config.state_filter);
    // filter.run();

    Ok(())
}
