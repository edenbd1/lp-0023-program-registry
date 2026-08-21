// SPDX-License-Identifier: MIT OR Apache-2.0
//! A self-hostable search index for a LEZ program registry.
//!
//! **Search is a convenience. Resolution is not.** Anything found here can be
//! re-derived from the program id alone and re-read straight from the chain, and
//! the `source` field on every result says exactly how. Run this because scanning
//! names is faster than deriving addresses one at a time, never because you have
//! to.
//!
//! That distinction is why this is a separate binary rather than a service the
//! registry depends on. If it is down, wrong, or lying, `lez-registry` and the
//! SDK still resolve every program without it.
//!
//! It holds no database. The index is rebuilt from the chain on start and on
//! `/refresh`, which keeps "self-hostable" honest: there is no state to migrate,
//! back up, or trust.

use lez_registry_core::{Entry, VerificationState};
use lez_registry_sdk::{base58, entry_address, Client};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};

const DEFAULT_ENDPOINT: &str = "https://testnet.lez.logos.co";

struct Index {
    entries: Vec<Entry>,
    registry: [u8; 32],
    endpoint: String,
}

impl Index {
    fn search(&self, q: &str) -> Vec<&Entry> {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return self.entries.iter().collect();
        }
        self.entries
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.version.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
                    || hex(&e.program_id).starts_with(&q)
                    || base58(&e.author).to_lowercase().contains(&q)
                    || state_word(&e.verification.state).contains(&q)
            })
            .collect()
    }
}

const fn state_word(s: &VerificationState) -> &'static str {
    match s {
        VerificationState::Unclaimed => "unclaimed",
        VerificationState::Claimed => "claimed",
        VerificationState::Verified => "verified",
        VerificationState::Mismatched => "mismatched",
        VerificationState::Unreproducible => "unreproducible",
    }
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn entry_json(e: &Entry, idx: &Index) -> String {
    let addr = base58(&entry_address(&idx.registry, &e.program_id));
    // Every result carries the two facts needed to check it without this
    // service: where the entry lives, and that the address is derived rather
    // than looked up.
    format!(
        r#"{{"name":"{}","version":"{}","description":"{}","program_id":"{}","author":"{}","verification":"{}","self_attested":{},"frozen":{},"registered_block":{},"last_checked_block":{},"entry_account":"{}","source":"derive pda(registry, sha256(\"lez-registry:entry:v1\" || program_id)) and read it; this index is not required"}}"#,
        escape(&e.name),
        escape(&e.version),
        escape(&e.description),
        hex(&e.program_id),
        base58(&e.author),
        state_word(&e.verification.state),
        e.verification.is_self_attested(&e.author),
        e.frozen,
        e.registered_at.block,
        e.verification.last_checked.map_or(0, |s| s.block),
        addr,
    )
}

fn refresh(client: &Client, index: &Arc<RwLock<Index>>) -> Result<usize, String> {
    let entries = client.walk().map_err(|e| e.to_string())?;
    let n = entries.len();
    index.write().map_err(|_| "index lock poisoned")?.entries = entries;
    Ok(n)
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn handle(mut stream: TcpStream, client: &Client, index: &Arc<RwLock<Index>>) {
    let mut line = String::new();
    if BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    })
    .read_line(&mut line)
    .is_err()
    {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();

    let idx = match index.read() {
        Ok(i) => i,
        Err(_) => {
            respond(
                &mut stream,
                "500 Internal Server Error",
                "text/plain",
                "index lock poisoned",
            );
            return;
        }
    };

    if path.starts_with("/search") {
        let q = path
            .split_once('?')
            .and_then(|(_, qs)| {
                qs.split('&')
                    .find_map(|kv| kv.strip_prefix("q=").map(url_decode))
            })
            .unwrap_or_default();
        let hits = idx.search(&q);
        let body = format!(
            r#"{{"query":"{}","count":{},"results":[{}]}}"#,
            escape(&q),
            hits.len(),
            hits.iter()
                .map(|e| entry_json(e, &idx))
                .collect::<Vec<_>>()
                .join(",")
        );
        respond(&mut stream, "200 OK", "application/json", &body);
        return;
    }

    if path == "/stats" {
        let verified = idx
            .entries
            .iter()
            .filter(|e| e.verification.is_verified())
            .count();
        let body = format!(
            r#"{{"registry":"{}","endpoint":"{}","indexed":{},"verified":{}}}"#,
            hex(&idx.registry),
            escape(&idx.endpoint),
            idx.entries.len(),
            verified
        );
        respond(&mut stream, "200 OK", "application/json", &body);
        return;
    }

    if path == "/refresh" {
        drop(idx);
        match refresh(client, index) {
            Ok(n) => respond(
                &mut stream,
                "200 OK",
                "application/json",
                &format!(r#"{{"indexed":{n}}}"#),
            ),
            Err(e) => respond(
                &mut stream,
                "502 Bad Gateway",
                "application/json",
                &format!(r#"{{"error":"{}"}}"#, escape(&e)),
            ),
        }
        return;
    }

    respond(
        &mut stream,
        "200 OK",
        "text/plain",
        "lez-registry-indexer\n\n  /search?q=<text>   name, version, description, tags, author, program id prefix, state\n  /stats             what is indexed\n  /refresh           rebuild from the chain\n\nThis index is an accelerator for browsing. Every result can be re-derived\nfrom the program id and read straight from the chain without it.\n",
    );
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hi = (b[i + 1] as char).to_digit(16);
                let lo = (b[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim().trim_start_matches("0x");
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(registry) = args.first().and_then(|s| parse_hex32(s)) else {
        eprintln!(
            "usage: lez-registry-indexer <REGISTRY_PROGRAM_ID_HEX> [--endpoint URL] [--port N]"
        );
        return std::process::ExitCode::FAILURE;
    };
    let endpoint = flag(&args, "--endpoint").unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let port: u16 = flag(&args, "--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);

    let client = Client::new(endpoint.clone(), registry);
    let index = Arc::new(RwLock::new(Index {
        entries: Vec::new(),
        registry,
        endpoint: endpoint.clone(),
    }));

    match refresh(&client, &index) {
        Ok(n) => println!("indexed {n} entries from {endpoint}"),
        // A failed first refresh is not fatal: the server comes up empty and
        // /refresh can be retried. Refusing to start would make a transient RPC
        // outage look like a broken deployment.
        Err(e) => eprintln!("initial refresh failed ({e}); starting empty, try /refresh"),
    }

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot bind 127.0.0.1:{port}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!("listening on http://127.0.0.1:{port}");
    for stream in listener.incoming().flatten() {
        handle(stream, &client, &index);
    }
    std::process::ExitCode::SUCCESS
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
