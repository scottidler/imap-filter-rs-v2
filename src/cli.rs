// src/cli/cli.rs

use clap::Parser;
use std::path::PathBuf;
use secure_string::SecureString;

/// Command-line interface options for imap-filter.
#[derive(Parser, Debug)]
#[command(
    name = "imap-filter",
    version = env!("GIT_DESCRIBE"),
    about = "IMAP email filtering CLI",
    long_about = None
)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "imap-filter.yml")]
    pub config: PathBuf,

    /// IMAP server domain
    #[arg(short = 'D', long, env = "IMAP_DOMAIN")]
    pub imap_domain: Option<String>,

    /// IMAP username
    #[arg(short = 'U', long, env = "IMAP_USERNAME")]
    pub imap_username: Option<String>,

    /// IMAP password
    #[arg(short = 'P', long, env = "IMAP_PASSWORD")]
    pub imap_password: Option<SecureString>,

    #[arg(short, long, help = "turn on client.debug logging")]
    pub debug: bool,
}
