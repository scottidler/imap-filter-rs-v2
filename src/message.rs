// src/message.rs

use std::collections::HashMap;
use mailparse::{addrparse, MailAddr};

use crate::cfg::label::Label;

#[derive(Debug)]
pub struct EmailAddress {
    pub name: String,
    pub email: String,
}

#[derive(Debug)]
pub struct Message {
    pub uid: u32,
    pub seq: u32,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub from: Vec<EmailAddress>,
    pub subject: String,
    pub date: String,
    pub labels: Vec<Label>,
    pub headers: HashMap<String, String>,
}

impl Message {
    pub fn new(
        uid: u32,
        seq: u32,
        raw_headers: Vec<u8>,
        raw_labels: Vec<String>,
        internal_date: String,
    ) -> Self {
        // parse headers
        let raw_str = String::from_utf8_lossy(&raw_headers);
        let headers: HashMap<_, _> = raw_str
            .lines()
            .filter_map(|line| line.split_once(": "))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // owned parsing of address fields
        let to = parse_addrs(headers.get("To"));
        let cc = parse_addrs(headers.get("Cc"));
        let from = parse_addrs(headers.get("From"));

        // labels and subject
        let labels = raw_labels.into_iter().map(|s| Label::new(&s)).collect();
        let subject = headers.get("Subject").cloned().unwrap_or_default();

        Message {
            uid,
            seq,
            to,
            cc,
            from,
            subject,
            date: internal_date,
            labels,
            headers,
        }
    }
}


/// Owned parsing of an address header into `EmailAddress`
fn parse_addrs(field: Option<&String>) -> Vec<EmailAddress> {
    if let Some(s) = field {
        if let Ok(addrs) = addrparse(s) {
            let mut result = Vec::new();
            for addr in addrs.iter() {
                match addr {
                    MailAddr::Single(info) => {
                        result.push(EmailAddress {
                            name: info.display_name.clone().unwrap_or_default(),
                            email: info.addr.clone(),
                        });
                    }
                    MailAddr::Group(group) => {
                        for info in &group.addrs {
                            result.push(EmailAddress {
                                name: info.display_name.clone().unwrap_or_default(),
                                email: info.addr.clone(),
                            });
                        }
                    }
                }
            }
            return result;
        }
    }
    Vec::new()
}
