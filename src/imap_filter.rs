// src/imap_filter.rs

use eyre::Result;
use imap::Session;
use log::{debug, info, error};
use native_tls::TlsStream;
use std::net::TcpStream;
use crate::cfg::message_filter::{MessageFilter, FilterAction};
use crate::cfg::state_filter::{StateFilter, StateAction, TTL};

pub struct IMAPFilter {
    pub client: Session<TlsStream<TcpStream>>,
    pub filters: Vec<MessageFilter>,
    pub states: Vec<StateFilter>,
}

impl IMAPFilter {
    pub fn new(
        client: Session<TlsStream<TcpStream>>,
        filters: Vec<MessageFilter>,
        states: Vec<StateFilter>,
    ) -> Self {
        IMAPFilter { client, filters, states }
    }

    // TODO: add your filter‐execution methods here
}
