// src/utils.rs
use eyre::{Result, eyre};
use imap::Session;
use native_tls::TlsStream;
use std::net::TcpStream;
use log::{info, debug};
use std::collections::HashSet;
use chrono::{DateTime, Duration, Utc};
use std::io::{Read, Write};
use regex::Regex;
