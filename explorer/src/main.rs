// SPDX-License-Identifier: MIT OR Apache-2.0
//! A public, self-hostable block explorer for a LEZ program registry.
//!
//! WHAT THIS IS, AND WHY IT CANNOT CENTRALISE THE REGISTRY
//!
//! It is a pure **view**. Every page is rendered from a live read of the chain
//! through `lez-registry-sdk` — the same derivation and decoding the CLI and the
//! on-chain program use — and it keeps no database. The only state it holds is an
//! in-memory, time-bounded cache that a restart rebuilds from the chain, and that
//! nothing resolves *from*: it is a latency shim, never a source of truth. Point
//! two instances at the same registry and endpoint and they render identical
//! bytes, because the data was never theirs. Turn this off and the registry is
//! untouched: `lez-registry lookup <id>` and the SDK resolve every program
//! without it. Run your own with one command; see `README.md`.
//!
//! The HTTP server is std-only, in the spirit of the `indexer` next door. The one
//! outbound dependency is an optional read to a Logos Storage gateway to resolve
//! an IDL — and when no gateway is configured the content id is still shown, so
//! the on-chain metadata never depends on it.

mod render;

use lez_registry_core::{AccountId, Entry, GlobalState, ProgramId, Publisher, VerificationState};
use lez_registry_sdk::{self as sdk, Client};
use render::{IdlView, Site};

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_REGISTRY: &str = "f59751e932e856f7565f34e79ae726791794dc68e55f2413bc0375008cc07bf8";
const DEFAULT_ENDPOINT: &str = "https://testnet.lez.logos.co";

struct Config {
    registry: ProgramId,
    registry_hex: String,
    endpoint: String,
    storage: Option<String>,
    cache_ttl: Option<Duration>,
}

impl Config {
    fn site(&self) -> Site<'_> {
        Site {
            registry: self.registry,
            registry_hex: &self.registry_hex,
            endpoint: &self.endpoint,
        }
    }
    fn client(&self) -> Client {
        Client::new(self.endpoint.clone(), self.registry)
    }
}

/// The whole registry, as of one walk. Held only to spare repeat visitors a fresh
/// walk within the TTL; it is discarded on restart and re-derivable at any time.
#[derive(Default)]
struct Cache {
    at: Option<Instant>,
    entries: Vec<Entry>,
    global: Option<GlobalState>,
}

struct App {
    cfg: Config,
    cache: Mutex<Cache>,
}

impl App {
    /// The registry as a list, from the cache when fresh, otherwise from a walk.
    fn snapshot(&self) -> Result<(Vec<Entry>, Option<GlobalState>), String> {
        if let Some(ttl) = self.cfg.cache_ttl {
            if let Ok(c) = self.cache.lock() {
                if let Some(at) = c.at {
                    if at.elapsed() < ttl {
                        return Ok((c.entries.clone(), c.global));
                    }
                }
            }
        }
        let client = self.cfg.client();
        let entries = client.walk().map_err(|e| e.to_string())?;
        let global = client.global().ok().flatten();
        if self.cfg.cache_ttl.is_some() {
            if let Ok(mut c) = self.cache.lock() {
                c.at = Some(Instant::now());
                c.entries = entries.clone();
                c.global = global;
            }
        }
        Ok((entries, global))
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return std::process::ExitCode::SUCCESS;
    }

    // Registry may be the first positional arg or --registry; default is the
    // live testnet deployment this repo documents.
    let reg_hex = flag(&args, "--registry")
        .or_else(|| args.first().filter(|a| !a.starts_with('-')).cloned())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string());
    let Some(registry) = from_hex(&reg_hex) else {
        eprintln!("registry must be 64 hex characters");
        return std::process::ExitCode::FAILURE;
    };
    let endpoint = flag(&args, "--endpoint").unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let port: u16 = flag(&args, "--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let storage = flag(&args, "--storage-gateway");
    let cache_ttl = if args.iter().any(|a| a == "--no-cache") {
        None
    } else {
        Some(Duration::from_secs(
            flag(&args, "--cache-ttl")
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
        ))
    };

    let app = Arc::new(App {
        cfg: Config {
            registry,
            registry_hex: hex(&registry),
            endpoint,
            storage,
            cache_ttl,
        },
        cache: Mutex::new(Cache::default()),
    });

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot bind 127.0.0.1:{port}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    println!("LEZ Registry Explorer");
    println!("  registry  {}", app.cfg.registry_hex);
    println!("  endpoint  {}", app.cfg.endpoint);
    match &app.cfg.storage {
        Some(g) => println!("  storage   {g}"),
        None => println!(
            "  storage   (none) — IDLs show their content id; pass --storage-gateway to resolve"
        ),
    }
    println!(
        "  cache     {}",
        match app.cfg.cache_ttl {
            Some(d) => format!(
                "{}s (in-memory, rebuilt from chain, never a source of truth)",
                d.as_secs()
            ),
            None => "off".to_string(),
        }
    );
    // Warm the cache so the first visitor is not the one who waits for the walk.
    match app.snapshot() {
        Ok((e, _)) => println!("  warmed    {} entries", e.len()),
        Err(e) => println!("  warmed    failed ({e}); pages will retry on demand"),
    }
    println!("\n  open http://127.0.0.1:{port}\n");

    for stream in listener.incoming().flatten() {
        let app = Arc::clone(&app);
        std::thread::spawn(move || handle(stream, &app));
    }
    std::process::ExitCode::SUCCESS
}

fn handle(mut stream: TcpStream, app: &App) {
    let peer = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(peer);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" {
        respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"only GET",
        );
        return;
    }

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };

    let site = app.cfg.site();

    if path == "/" {
        match app.snapshot() {
            Ok((entries, global)) => {
                let html = render::list_page(&site, &entries, global.as_ref());
                respond_html(&mut stream, "200 OK", &html);
            }
            Err(e) => respond_html(
                &mut stream,
                "502 Bad Gateway",
                &render::not_found(
                    &site,
                    "Cannot reach the chain",
                    &format!("The sequencer at <span class=\"mono\">{}</span> did not answer: {}. This is a read against the chain — retry, or point <span class=\"mono\">--endpoint</span> elsewhere.", esc_txt(&app.cfg.endpoint), esc_txt(&e)),
                ),
            ),
        }
        return;
    }

    if path == "/search" {
        let q = query_param(query, "q").unwrap_or_default();
        match app.snapshot() {
            Ok((entries, _)) => {
                let hits = search(&entries, &q);
                let html = render::search_page(&site, &q, &hits, entries.len());
                respond_html(&mut stream, "200 OK", &html);
            }
            Err(e) => respond_html(
                &mut stream,
                "502 Bad Gateway",
                &render::not_found(&site, "Cannot reach the chain", &esc_txt(&e)),
            ),
        }
        return;
    }

    if let Some(rest) = path.strip_prefix("/program/") {
        serve_detail(&mut stream, app, &site, rest, query);
        return;
    }

    if path == "/api/programs.json" {
        match app.snapshot() {
            Ok((entries, global)) => {
                respond(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    programs_json(app, &entries, global.as_ref()).as_bytes(),
                );
            }
            Err(e) => respond(
                &mut stream,
                "502 Bad Gateway",
                "application/json; charset=utf-8",
                format!(r#"{{"error":{}}}"#, json_str(&e)).as_bytes(),
            ),
        }
        return;
    }

    if path == "/health" {
        respond(&mut stream, "200 OK", "text/plain; charset=utf-8", b"ok");
        return;
    }

    respond_html(
        &mut stream,
        "404 Not Found",
        &render::not_found(
            &site,
            "Not found",
            "No such page. Try the <a href=\"/\">registry list</a> or a program id at <span class=\"mono\">/program/&lt;hex&gt;</span>.",
        ),
    );
}

fn serve_detail(stream: &mut TcpStream, app: &App, site: &Site, rest: &str, query: &str) {
    let Some(pid) = from_hex(rest.trim_end_matches('/')) else {
        respond_html(
            stream,
            "400 Bad Request",
            &render::not_found(
                site,
                "Not a program id",
                "A program id is 64 hex characters — the RISC0 ImageID a program is called by.",
            ),
        );
        return;
    };
    let publisher: Option<AccountId> = query_param(query, "publisher").and_then(|s| from_hex(&s));

    let client = app.cfg.client();
    let fetched = match &publisher {
        Some(p) => client.third_party_entry(&pid, p),
        None => client.entry(&pid),
    };

    match fetched {
        Ok(Some(e)) => {
            let idl = resolve_idl(app, &e);
            let html = render::detail_page(site, &e, publisher.as_ref(), &idl);
            respond_html(stream, "200 OK", &html);
        }
        Ok(None) => {
            let addr = sdk::base58(&match &publisher {
                Some(p) => sdk::third_party_entry_address(&app.cfg.registry, &pid, p),
                None => sdk::entry_address(&app.cfg.registry, &pid),
            });
            respond_html(
                stream,
                "404 Not Found",
                &render::not_found(
                    site,
                    "Nothing registered there",
                    &format!(
                        "The address <span class=\"mono\">{}</span> is well-formed and derivable from this program id, but nothing has been written to it. A derived address always exists; what makes a program registered is whether an entry was ever written.",
                        esc_txt(&addr)
                    ),
                ),
            );
        }
        Err(e) => respond_html(
            stream,
            "502 Bad Gateway",
            &render::not_found(site, "Cannot reach the chain", &esc_txt(&e.to_string())),
        ),
    }
}

/// Turn an entry's on-chain IDL pointer into something to render, fetching from a
/// Storage gateway only if one was configured. Never fatal: a missing or bad IDL
/// leaves the on-chain metadata untouched.
fn resolve_idl(app: &App, e: &Entry) -> IdlView {
    let Some(cid_bytes) = e.idl else {
        return IdlView::None;
    };
    let cid = render::content_id_string(&cid_bytes);
    let Some(gateway) = &app.cfg.storage else {
        return IdlView::Present {
            cid,
            resolved: None,
        };
    };
    let url = format!("{}/{}", gateway.trim_end_matches('/'), cid);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build();
    let resolved = match agent.get(&url).call() {
        Ok(resp) => {
            let mut body = String::new();
            // Cap the read: an IDL is kilobytes, and an explorer should not stream
            // an unbounded blob into a page because a gateway offered one.
            match resp.into_reader().take(256 * 1024).read_to_string(&mut body) {
                Ok(_) if json_wellformed(&body) => Ok(truncate(&body, 8000)),
                Ok(_) => Err("resolved, but the bytes do not parse as JSON — treated as unavailable rather than trusting a partial parse".into()),
                Err(err) => Err(format!("read failed: {err}")),
            }
        }
        Err(err) => Err(format!("gateway {url} did not answer: {err}")),
    };
    IdlView::Present {
        cid,
        resolved: Some(resolved),
    }
}

// ---------------------------------------------------------------------------
// search — the same predicate the SDK's `search` and the indexer apply
// ---------------------------------------------------------------------------

fn state_word(s: &VerificationState) -> &'static str {
    match s {
        VerificationState::Unclaimed => "unclaimed",
        VerificationState::Claimed => "claimed",
        VerificationState::Verified => "verified",
        VerificationState::Mismatched => "mismatched",
        VerificationState::Unreproducible => "unreproducible",
    }
}

fn search(entries: &[Entry], q: &str) -> Vec<Entry> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&q)
                || e.version.to_lowercase().contains(&q)
                || e.description.to_lowercase().contains(&q)
                || e.tags.iter().any(|t| t.to_lowercase().contains(&q))
                || hex(&e.program_id).starts_with(q.trim_start_matches("0x"))
                || sdk::base58(&e.author).to_lowercase().contains(&q)
                || state_word(&e.verification.state).contains(&q)
                || (q == "canonical" && matches!(e.publisher, Publisher::Canonical))
                || (q.starts_with("third") && matches!(e.publisher, Publisher::ThirdParty))
                || (q == "self-attested" && e.verification.is_self_attested(&e.author))
                || (q == "frozen" && e.frozen)
        })
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// JSON API — a machine-readable mirror of the list, so the view is scriptable
// and its numbers checkable. Hand-built to avoid a serialisation dependency.
// ---------------------------------------------------------------------------

fn programs_json(app: &App, entries: &[Entry], global: Option<&GlobalState>) -> String {
    let mut items = Vec::with_capacity(entries.len());
    for e in entries {
        let addr = sdk::base58(&sdk::entry_address(&app.cfg.registry, &e.program_id));
        items.push(format!(
            r#"{{"name":{name},"version":{ver},"program_id":{pid},"author":{author},"publisher":{publisher},"verification":{state},"self_attested":{self_att},"frozen":{frozen},"registered_block":{block},"entry_account":{acct},"note":"re-derive: pda(registry, sha256(\"lez-registry:entry:v1\" || program_id)); this API is not required"}}"#,
            name = json_str(&e.name),
            ver = json_str(&e.version),
            pid = json_str(&hex(&e.program_id)),
            author = json_str(&sdk::base58(&e.author)),
            publisher = json_str(match e.publisher { Publisher::Canonical => "canonical", Publisher::ThirdParty => "third-party" }),
            state = json_str(state_word(&e.verification.state)),
            self_att = e.verification.is_self_attested(&e.author),
            frozen = e.frozen,
            block = e.registered_at.block,
            acct = json_str(&addr),
        ));
    }
    format!(
        r#"{{"registry":{reg},"endpoint":{ep},"total_on_chain":{total},"total_verified":{ver},"walked":{walked},"programs":[{items}]}}"#,
        reg = json_str(&app.cfg.registry_hex),
        ep = json_str(&app.cfg.endpoint),
        total = global.map_or(entries.len() as u64, |g| g.total_entries),
        ver = global.map_or(0, |g| g.total_verified),
        walked = entries.len(),
        items = items.join(","),
    )
}

// ---------------------------------------------------------------------------
// tiny utilities
// ---------------------------------------------------------------------------

fn hex(b: &[u8]) -> String {
    let mut o = String::with_capacity(b.len() * 2);
    for x in b {
        o.push_str(&format!("{x:02x}"));
    }
    o
}

fn from_hex(s: &str) -> Option<ProgramId> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| url_decode(v))
    })
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

/// A minimal, dependency-free JSON well-formedness check — enough to tell a
/// usable IDL from an unusable one without pulling in a parser.
fn json_wellformed(text: &str) -> bool {
    let text = text.trim();
    if !(text.starts_with('{') || text.starts_with('[')) {
        return false;
    }
    let (mut depth, mut in_string, mut escaped) = (0i32, false, false);
    for c in text.chars() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_string
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(max).collect();
        o.push_str("\n… (truncated; fetch the full object from the gateway)");
        o
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Plain-text escape for interpolating an untrusted string into a notice's HTML.
fn esc_txt(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn respond_html(stream: &mut TcpStream, status: &str, html: &str) {
    respond(stream, status, "text/html; charset=utf-8", html.as_bytes());
}

fn print_usage() {
    println!(
        "\
lez-registry-explorer — a pure, self-hostable view over a LEZ program registry

USAGE
  lez-registry-explorer [REGISTRY_HEX] [options]

OPTIONS
  --registry <HEX>          registry program id (default: the documented testnet deployment)
  --endpoint <URL>          sequencer JSON-RPC endpoint (default: {ep})
  --port <N>                listen port (default: 8080)
  --storage-gateway <URL>   Logos Storage gateway to resolve IDL content ids (optional)
  --cache-ttl <SECONDS>     in-memory walk cache TTL (default: 10)
  --no-cache                read the chain on every request
  -h, --help                this message

It holds no database. Everything it serves is read live from the chain and is
re-derivable without it: turn it off and `lez-registry lookup <id>` still works.",
        ep = DEFAULT_ENDPOINT
    );
}
