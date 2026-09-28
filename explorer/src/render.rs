// SPDX-License-Identifier: MIT OR Apache-2.0
//! HTML for the explorer.
//!
//! This module renders and nothing else: it performs no network I/O and holds
//! no state. Every address it prints is computed with the same
//! `lez-registry-core` / `lez-registry-sdk` functions the on-chain program and
//! the CLI use, so a reader can reproduce any of it by hand. The point of the
//! derivation trace on the detail page is exactly that — to show its own work.

use lez_registry_core as core_;
use lez_registry_sdk as sdk;

use core_::{AccountId, Entry, GlobalState, ProgramId, Publisher, Stamp, VerificationState};

/// The one deployment this instance is a view of. Everything shown is scoped to
/// it, and it is printed on every page so nobody has to trust the wordmark.
pub struct Site<'a> {
    pub registry: ProgramId,
    pub registry_hex: &'a str,
    pub endpoint: &'a str,
    /// The origin the visitor reached this instance on (`https://host[:port]`),
    /// used to make permalinks and social-card URLs absolute. `None` when no
    /// usable `Host` header was sent; the page then omits absolute-URL tags
    /// rather than inventing an address.
    pub origin: Option<String>,
    /// The explorer's *own* source, so the site can be rebuilt and checked
    /// against what is running. These are the explorer verifying itself.
    pub repo: &'a str,
    pub commit: &'a str,
    pub self_digest: &'a str,
}

impl Site<'_> {
    /// An absolute URL for `path` when the origin is known, else the path itself.
    fn abs(&self, path: &str) -> Option<String> {
        self.origin.as_ref().map(|o| format!("{o}{path}"))
    }
}

/// What the handler managed to do with an entry's IDL pointer, kept as data so
/// this module stays free of I/O.
pub enum IdlView {
    /// No IDL was published on chain.
    None,
    /// A CID is on chain. `resolved` is `None` when no gateway was configured,
    /// `Some(Ok(pretty_json))` when it fetched and parsed, `Some(Err(reason))`
    /// when a fetch was attempted and could not be trusted.
    Present {
        cid: String,
        resolved: Option<Result<String, String>>,
    },
}

const STYLE: &str = include_str!("../assets/style.css");
const APP_JS: &str = include_str!("../assets/app.js");

// ---------------------------------------------------------------------------
// small pure helpers
// ---------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn hex(b: &[u8]) -> String {
    let mut o = String::with_capacity(b.len() * 2);
    for x in b {
        o.push_str(&format!("{x:02x}"));
    }
    o
}

/// base58btc over arbitrary-length bytes — used to render a 38-byte content id.
/// `sdk::base58` is fixed at 32 bytes; a content id is not.
fn base58btc(bytes: &[u8]) -> String {
    const A: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut digits: Vec<u8> = Vec::new();
    for &b in bytes {
        let mut carry = b as u32;
        for d in digits.iter_mut() {
            carry += (*d as u32) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let leading = bytes.iter().take_while(|b| **b == 0).count();
    let mut out = String::with_capacity(leading + digits.len());
    for _ in 0..leading {
        out.push('1');
    }
    for d in digits.iter().rev() {
        out.push(A[*d as usize] as char);
    }
    out
}

/// The multibase base58btc form a Storage node hands you: a leading `z`.
pub fn content_id_string(cid: &[u8]) -> String {
    format!("z{}", base58btc(cid))
}

fn short_hex(b: &[u8]) -> String {
    let h = hex(b);
    if h.len() <= 20 {
        h
    } else {
        format!("{}…{}", &h[..8], &h[h.len() - 8..])
    }
}

fn short_b58(s: &str) -> String {
    if s.len() <= 16 {
        s.to_string()
    } else {
        format!("{}…{}", &s[..6], &s[s.len() - 6..])
    }
}

/// Epoch milliseconds to `YYYY-MM-DD HH:MM UTC`, no date crate. The civil-date
/// arithmetic is Howard Hinnant's `civil_from_days`.
fn fmt_millis(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, mi) = (tod / 3600, (tod % 3600) / 60);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02} UTC")
}

fn stamp_line(s: &Stamp) -> String {
    format!(
        "block {} · {}",
        s.block,
        if s.millis == 0 {
            "no timestamp".to_string()
        } else {
            fmt_millis(s.millis)
        }
    )
}

fn copy_btn(value: &str) -> String {
    format!(
        r#"<button class="copy" type="button" data-copy="{}">copy</button>"#,
        esc(value)
    )
}

// ---------------------------------------------------------------------------
// verification-state vocabulary — one hue and one sentence per state, and the
// self-attested case rendered apart from an independent one on purpose.
// ---------------------------------------------------------------------------

struct StateFace {
    class: &'static str,
    label: String,
    sentence: &'static str,
}

fn state_face(e: &Entry) -> StateFace {
    match e.verification.state {
        VerificationState::Unclaimed => StateFace {
            class: "v-unclaimed",
            label: "Unclaimed".into(),
            sentence:
                "Nobody has said where the source is. The honest default, not a failure.",
        },
        VerificationState::Claimed => StateFace {
            class: "v-claimed",
            label: "Claimed".into(),
            sentence:
                "A source is asserted and nobody has rebuilt it. Typing a URL is not evidence.",
        },
        VerificationState::Unreproducible => StateFace {
            class: "v-unrepro",
            label: "Unreproducible".into(),
            sentence:
                "A rebuild was recorded, but the claimed build had network access, so a match would prove nothing about the source.",
        },
        VerificationState::Mismatched => StateFace {
            class: "v-mismatch",
            label: "Mismatched".into(),
            sentence:
                "A source was published and does not rebuild to this bytecode. A stronger negative signal than no claim at all — not merely unverified.",
        },
        VerificationState::Verified => {
            if e.verification.is_self_attested(&e.author) {
                StateFace {
                    class: "v-verified v-self",
                    label: "Verified · self-attested".into(),
                    sentence:
                        "An offline rebuild reproduced the deployed bytecode — but the only party vouching for it is the party that shipped it. Useful, and not an independent check.",
                }
            } else {
                StateFace {
                    class: "v-verified",
                    label: "Verified".into(),
                    sentence:
                        "Another party ran an offline rebuild and it reproduced the deployed bytecode. What it proves: this bytecode comes from that source. Not that it is safe.",
                }
            }
        }
    }
}

fn badge(e: &Entry) -> String {
    let f = state_face(e);
    format!(
        r#"<span class="badge {}"><span class="dot"></span>{}</span>"#,
        f.class,
        esc(&f.label)
    )
}

fn publisher_tag(p: &Publisher) -> String {
    match p {
        Publisher::Canonical => {
            r#"<span class="pub canonical" title="Published by an account that proved it can rebuild the deployed bytecode.">canonical</span>"#.to_string()
        }
        Publisher::ThirdParty => {
            r#"<span class="pub" title="Published by another party. Not lesser, not greater — different.">third&#8209;party</span>"#.to_string()
        }
    }
}

/// The link to an entry's own page. A third-party entry shares a program id with
/// the canonical one, so it must carry the publisher to resolve back to itself.
fn detail_href(e: &Entry) -> String {
    match e.publisher {
        Publisher::Canonical => format!("/program/{}", hex(&e.program_id)),
        Publisher::ThirdParty => {
            format!(
                "/program/{}?publisher={}",
                hex(&e.program_id),
                hex(&e.author)
            )
        }
    }
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------

fn header(site: &Site, query: &str) -> String {
    format!(
        r#"<header class="site-header"><div class="wrap">
  <a class="brand" href="/"><span class="mark">LEZ</span><span class="name">Registry Explorer</span><span class="sub">block explorer</span></a>
  <div class="header-search"><form action="/search" method="get" role="search">
    <input type="search" name="q" value="{q}" placeholder="Search name, author, program id, state…" aria-label="Search the registry" autocomplete="off">
  </form></div>
  <a class="nav-link" href="/adoption">Adoption</a>
  <a class="nav-link" href="/about">About</a>
  <button id="theme-toggle" class="theme-toggle" type="button">◑ dark</button>
</div></header>
<div class="provenance"><div class="wrap">
  <span class="claim"><b>Pure view.</b> No database — every value is read live from the chain and re-derivable without this site.</span>
  <span class="kv"><span class="k">registry</span><span class="v" title="{reg_full}">{reg_short}</span></span>
  <span class="kv"><span class="k">endpoint</span><span class="v">{ep}</span></span>
</div></div>"#,
        q = esc(query),
        reg_full = esc(site.registry_hex),
        reg_short = esc(&short_hex(&site.registry)),
        ep = esc(site.endpoint),
    )
}

fn footer(site: &Site) -> String {
    format!(
        r#"<footer class="site-footer"><div class="wrap">
  <span>A self-hostable view over the LEZ program registry. Reads only; signs nothing; stores nothing. <a href="/about">How this stays decentralised →</a></span>
  <span>registry <span class="mono">{reg}</span> · <a href="/api/programs.json">JSON</a></span>
  <span class="self-verify" title="The explorer verifies itself: rebuild this commit reproducibly and the sha256 matches.">this build <span class="mono">{commit}</span> · binary sha256 <span class="mono">{digest}</span> · <a href="/about#self-verify">rebuild &amp; confirm</a></span>
</div></footer>"#,
        reg = esc(site.registry_hex),
        commit = esc(&short_commit(site.commit)),
        digest = esc(&short_hex_str(site.self_digest)),
    )
}

/// A commit shown short when it is a full hash, verbatim when it is the
/// "unpinned…" note so the reader is not misled into thinking it is a real sha.
fn short_commit(commit: &str) -> String {
    let is_hex = commit.len() >= 40 && commit.chars().all(|c| c.is_ascii_hexdigit());
    if is_hex {
        format!("{}…", &commit[..10])
    } else {
        commit.to_string()
    }
}

/// Shorten a hex string (already a string, e.g. the binary digest) for display.
fn short_hex_str(h: &str) -> String {
    if h.len() <= 20 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        h.to_string()
    } else {
        format!("{}…{}", &h[..8], &h[h.len() - 8..])
    }
}

/// Per-page social/SEO metadata. Kept dependency-light: every tag is a static
/// string plus values already read from the chain — nothing here fetches or
/// embeds an external resource, so the "pure view" claim survives a shared link.
pub struct Meta {
    /// `<meta name="description">` and the OG/Twitter description.
    pub description: String,
    /// og:title / twitter:title. Falls back to the page `<title>` when empty.
    pub og_title: String,
    /// The path (e.g. `/program/<hex>`) for `<link rel=canonical>` and og:url,
    /// joined with the request origin. Absolute-URL tags are omitted when the
    /// origin is unknown rather than pointing at a guessed host.
    pub canonical_path: String,
}

impl Meta {
    fn generic(canonical_path: &str) -> Self {
        Self {
            description: String::new(),
            og_title: String::new(),
            canonical_path: canonical_path.to_string(),
        }
    }
}

/// The description used when a page does not set its own.
const SITE_DESCRIPTION: &str =
    "A pure, self-hostable view over the Logos Execution Zone program registry. \
     No database; every value is read live from the chain and re-derivable without this site.";

/// The `<head>` meta block: description, canonical link, and Open Graph / Twitter
/// card tags so a shared permalink renders a clean social card. A `summary` card
/// is used deliberately — it needs no raster image and so pulls in no external
/// asset, keeping the view pure.
fn head_meta(site: &Site, title: &str, meta: &Meta) -> String {
    let desc = if meta.description.is_empty() {
        SITE_DESCRIPTION.to_string()
    } else {
        meta.description.clone()
    };
    let og_title = if meta.og_title.is_empty() {
        title.to_string()
    } else {
        meta.og_title.clone()
    };
    let url_tags = match site.abs(&meta.canonical_path) {
        Some(u) => format!(
            "<link rel=\"canonical\" href=\"{u}\">\n<meta property=\"og:url\" content=\"{u}\">\n",
            u = esc(&u)
        ),
        None => String::new(),
    };
    format!(
        r#"<meta name="description" content="{desc}">
{url_tags}<meta property="og:type" content="website">
<meta property="og:site_name" content="LEZ Registry Explorer">
<meta property="og:title" content="{og_title}">
<meta property="og:description" content="{desc}">
<meta name="twitter:card" content="summary">
<meta name="twitter:title" content="{og_title}">
<meta name="twitter:description" content="{desc}">"#,
        desc = esc(&desc),
        url_tags = url_tags,
        og_title = esc(&og_title),
    )
}

pub fn page(site: &Site, title: &str, query: &str, body: &str) -> String {
    page_meta(site, title, query, &Meta::generic("/"), body)
}

pub fn page_meta(site: &Site, title: &str, query: &str, meta: &Meta, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en" data-theme="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark light">
{meta}
<title>{title}</title>
<style>{style}</style>
</head>
<body>
{header}
<main><div class="wrap">
{body}
</div></main>
{footer}
<script>{js}</script>
</body>
</html>"#,
        meta = head_meta(site, title, meta),
        title = esc(title),
        style = STYLE,
        header = header(site, query),
        body = body,
        footer = footer(site),
        js = APP_JS,
    )
}

// ---------------------------------------------------------------------------
// the five-state legend, shown once under the list
// ---------------------------------------------------------------------------

/// The `<div class="legend-grid">` of state badge + sentence, built from synthetic
/// entries so it renders identically to the live cells and can never drift from
/// them. Shared by the list legend and the /about states panel.
fn legend_grid_only() -> String {
    let states = [
        (VerificationState::Verified, false),
        (VerificationState::Verified, true),
        (VerificationState::Claimed, false),
        (VerificationState::Unclaimed, false),
        (VerificationState::Unreproducible, false),
        (VerificationState::Mismatched, false),
    ];
    let mut items = String::new();
    for (st, self_att) in states {
        // A fabricated entry, used only to render a badge + sentence identically
        // to the live cells. self_att=true is the verified-by-its-own-author face.
        let author: AccountId = [1u8; 32];
        let mut e = Entry::new(
            [0u8; 32],
            Stamp {
                block: 0,
                millis: 0,
            },
            "x".into(),
            "0".into(),
            author,
            Publisher::Canonical,
        )
        .expect("legend template entry");
        e.verification.state = st.clone();
        if matches!(st, VerificationState::Verified) {
            e.verification.attester = Some(if self_att { author } else { [2u8; 32] });
        }
        let f = state_face(&e);
        items.push_str(&format!(
            r#"<div class="legend-item">{badge}<p>{sentence}</p></div>"#,
            badge = badge(&e),
            sentence = esc(f.sentence),
        ));
    }
    format!(r#"<div class="legend-grid">{items}</div>"#, items = items)
}

fn legend() -> String {
    format!(
        r#"<section class="legend">
  <h2>What the states mean</h2>
  <p class="lead">Verified is the only green on this page, and it is earned rather than assumed. None of these is a tick: each says exactly what is known, and no more. <a href="/about#self-verify">Verified does not mean safe →</a></p>
  {grid}
</section>"#,
        grid = legend_grid_only(),
    )
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn rows(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return r#"<tr><td colspan="6" style="padding:34px;text-align:center;color:var(--text-faint)">No entries at this registry yet.</td></tr>"#.to_string();
    }
    let mut out = String::new();
    for e in entries {
        let href = detail_href(e);
        out.push_str(&format!(
            r#"<tr>
  <td class="cell-name"><a href="{href}">{name}</a>{frozen}</td>
  <td class="cell-ver">{ver}</td>
  <td class="cell-id"><a href="{href}" title="{id_full}">{id_short}</a></td>
  <td class="cell-author"><span title="{auth_full}">{auth_short}</span></td>
  <td>{publisher}</td>
  <td>{badge}</td>
</tr>"#,
            href = esc(&href),
            name = esc(&e.name),
            frozen = if e.frozen {
                r#" <span class="frozen-tag" title="Immutable — the author froze it.">❄ frozen</span>"#
            } else {
                ""
            },
            ver = esc(&e.version),
            id_full = esc(&hex(&e.program_id)),
            id_short = esc(&short_hex(&e.program_id)),
            auth_full = esc(&sdk::base58(&e.author)),
            auth_short = esc(&short_b58(&sdk::base58(&e.author))),
            publisher = publisher_tag(&e.publisher),
            badge = badge(e),
        ));
    }
    out
}

fn table(entries: &[Entry]) -> String {
    format!(
        r#"<div class="table-card"><div class="table-scroll"><table class="registry">
<thead><tr><th>Program</th><th>Version</th><th>Program&nbsp;ID</th><th>Author</th><th>Publisher</th><th>Verification</th></tr></thead>
<tbody>{rows}</tbody>
</table></div></div>"#,
        rows = rows(entries)
    )
}

pub fn list_page(site: &Site, entries: &[Entry], global: Option<&GlobalState>) -> String {
    let verified = entries
        .iter()
        .filter(|e| e.verification.is_verified())
        .count();
    let canonical = entries
        .iter()
        .filter(|e| matches!(e.publisher, Publisher::Canonical))
        .count();
    let (declared_total, declared_verified) = global
        .map_or((entries.len() as u64, verified as u64), |g| {
            (g.total_entries, g.total_verified)
        });

    let stats = format!(
        r#"<div class="stats">
  <div class="stat"><span class="n">{shown}</span><span class="l">Entries walked</span></div>
  <div class="stat accent"><span class="n">{verified}</span><span class="l">Verified</span></div>
  <div class="stat"><span class="n">{canonical}</span><span class="l">Canonical</span></div>
  <div class="stat"><span class="n">{total}</span><span class="l">On-chain counter</span></div>
</div>"#,
        shown = entries.len(),
        verified = declared_verified.max(verified as u64),
        canonical = canonical,
        total = declared_total,
    );

    let body = format!(
        r#"<span class="eyebrow">All registered programs</span>
<h1 class="page-title">Registry</h1>
<p class="detail-sub">Enumerated by walking the registry's own on-chain sequence directory — no account index exists on this chain, and none is trusted. Each row links to an entry you can re-derive and re-read yourself.</p>
{stats}
{table}
{legend}"#,
        stats = stats,
        table = table(entries),
        legend = legend(),
    );
    page(site, "LEZ Registry Explorer", "", &body)
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

pub fn search_page(site: &Site, query: &str, hits: &[Entry], total: usize) -> String {
    let count = hits.len();
    let heading = if query.trim().is_empty() {
        format!("All {total} entries")
    } else {
        format!("{count} of {total} entries match “{}”", esc(query))
    };
    let body = format!(
        r#"<span class="eyebrow">Search · name, author, program id, version, tag, state</span>
<h1 class="page-title">{heading}</h1>
<div class="searchbar-lg"><form action="/search" method="get" role="search">
  <input type="search" name="q" value="{q}" placeholder="e.g. antumbra, mismatched, canonical, an author id…" aria-label="Search the registry" autocomplete="off">
  <button type="submit">Search</button>
</form></div>
{table}
<p class="detail-sub" style="margin-top:18px">This filter runs over the same walk the SDK does, the same logic the open-source <span class="mono">indexer</span> exposes as a service. Every match is re-derivable from its program id without either.</p>"#,
        heading = heading,
        q = esc(query),
        table = table(hits),
    );
    page(site, "Search · LEZ Registry Explorer", query, &body)
}

// ---------------------------------------------------------------------------
// detail
// ---------------------------------------------------------------------------

/// The derivation trace: this page's signature. It shows, in the reader's face,
/// how the account it just read was computed from the program id — so the site
/// is demonstrably an accelerator and not an authority.
fn derivation_trace(site: &Site, e: &Entry, publisher: Option<&AccountId>) -> String {
    let (seed, seed_expr) = match publisher {
        None => (
            core_::entry_seed(&e.program_id),
            r#"sha256(<b>"lez-registry:entry:v1"</b> ‖ program_id)"#.to_string(),
        ),
        Some(p) => (
            core_::third_party_entry_seed(&e.program_id, p),
            format!(
                r#"sha256(<b>"lez-registry:entry:v1:third-party:"</b> ‖ program_id ‖ <span class="op">{}</span>)"#,
                esc(&short_b58(&sdk::base58(p)))
            ),
        ),
    };
    let addr = sdk::pda(&site.registry, &seed);
    let addr_b58 = sdk::base58(&addr);
    format!(
        r#"<div class="trace">
  <div class="trace-head"><span class="t">Derivation</span><span class="s">computed here, not looked up</span></div>
  <ol>
    <li><span class="op">program_id</span> = <b>{pid}</b></li>
    <li><span class="op">seed</span> = {seed_expr}<br>&nbsp;&nbsp;= {seed_hex}</li>
    <li><span class="op">account</span> = sha256(<b>"/LEE/v0.2/AccountId/PDA/…"</b> ‖ registry ‖ seed)<br>&nbsp;&nbsp;= <span class="out">{addr}</span></li>
  </ol>
  <div class="foot">Read it yourself — no explorer required: <code>getAccount {addr}</code> against <code>{ep}</code>, or <code>lez-registry lookup {pid}</code>.</div>
</div>"#,
        pid = esc(&hex(&e.program_id)),
        seed_expr = seed_expr,
        seed_hex = esc(&hex(&seed)),
        addr = esc(&addr_b58),
        ep = esc(site.endpoint),
    )
}

fn idl_panel(view: &IdlView) -> String {
    match view {
        IdlView::None => r#"<div class="panel"><h2>Interface (IDL)</h2><div class="body"><p class="dim">No IDL published for this entry. The on-chain metadata above is unaffected — an interface is optional.</p></div></div>"#.to_string(),
        IdlView::Present { cid, resolved } => {
            let resolved_html = match resolved {
                None => format!(
                    r#"<p class="dim" style="margin:12px 0 0">The IDL JSON lives on Logos Storage, not on chain — only this identifier does, because an on-chain kilobyte is paid for by everyone who syncs the zone. Point the explorer at a Storage gateway (<span class="mono">--storage-gateway URL</span>) to resolve it here, or fetch it with any Storage node:</p><pre class="code"><span class="c"># the identifier is well-formed; that is a different claim from the IDL being available</span>
lez-registry idl <b>{cid}</b> --from ./storage-dir</pre>"#,
                    cid = esc(cid)
                ),
                Some(Ok(json)) => format!(
                    r#"<p class="dim" style="margin:12px 0 8px">Resolved from the configured gateway and parsed as JSON:</p><pre class="code">{json}</pre>"#,
                    json = esc(json)
                ),
                Some(Err(reason)) => format!(
                    r#"<p class="dim" style="margin:12px 0 0">IDL unavailable: {reason}. The entry's on-chain metadata is unaffected — this is graceful degradation, not an error.</p>"#,
                    reason = esc(reason)
                ),
            };
            format!(
                r#"<div class="panel"><h2>Interface (IDL)</h2><div class="body">
  <div class="hash">{cid}{copy}</div>
  {resolved_html}
</div></div>"#,
                cid = esc(cid),
                copy = copy_btn(cid),
                resolved_html = resolved_html,
            )
        }
    }
}

pub fn detail_page(site: &Site, e: &Entry, publisher: Option<&AccountId>, idl: &IdlView) -> String {
    let f = state_face(e);
    let author_b58 = sdk::base58(&e.author);

    // State callout — the verdict, spelled out, with "verified ≠ safe" attached.
    let state_callout = format!(
        r#"<div class="state-callout {class}">{badge}<p>{sentence}</p></div>"#,
        class = f.class,
        badge = badge(e),
        sentence = esc(f.sentence),
    );

    // Metadata.
    let tags_html = if e.tags.is_empty() {
        r#"<span class="faint">none</span>"#.to_string()
    } else {
        let mut t = String::from(r#"<div class="tags">"#);
        for tag in &e.tags {
            t.push_str(&format!(r#"<span class="tag">{}</span>"#, esc(tag)));
        }
        t.push_str("</div>");
        t
    };

    let attester_row = match &e.verification.attester {
        Some(a) => format!(
            r#"<div><dt>Attester</dt><dd class="mono">{}{}</dd></div>"#,
            esc(&sdk::base58(a)),
            copy_btn(&sdk::base58(a)),
        ),
        None => String::new(),
    };
    let last_checked_row = match &e.verification.last_checked {
        Some(s) => format!(
            r#"<div><dt>Last checked</dt><dd>{}</dd></div>"#,
            esc(&stamp_line(s))
        ),
        None => r#"<div><dt>Last checked</dt><dd class="faint">never</dd></div>"#.to_string(),
    };

    let meta = format!(
        r#"<div class="panel"><h2>Entry</h2><div class="body"><dl class="kv">
  <div><dt>Program ID</dt><dd class="mono">{pid}{pid_copy}</dd></div>
  <div><dt>Version</dt><dd class="mono">{ver}</dd></div>
  <div><dt>Author</dt><dd class="mono">{author}{auth_copy}</dd></div>
  <div><dt>Publisher</dt><dd>{publisher}</dd></div>
  <div><dt>Registered</dt><dd>{registered}</dd></div>
  {attester}
  {last_checked}
  <div><dt>Tags</dt><dd>{tags}</dd></div>
  <div><dt>Mutability</dt><dd>{frozen}</dd></div>
</dl></div></div>"#,
        pid = esc(&hex(&e.program_id)),
        pid_copy = copy_btn(&hex(&e.program_id)),
        ver = esc(&e.version),
        author = esc(&author_b58),
        auth_copy = copy_btn(&author_b58),
        publisher = publisher_tag(&e.publisher),
        registered = esc(&stamp_line(&e.registered_at)),
        attester = attester_row,
        last_checked = last_checked_row,
        tags = tags_html,
        frozen = if e.frozen {
            r#"<span class="mono">frozen</span> — one-way; even the author can no longer change it"#
        } else {
            r#"<span class="mono">mutable</span> — the recorded author may amend it"#
        },
    );

    // Source claim panel.
    let source_panel = match &e.verification.claim {
        None => format!(
            r#"<div class="panel"><h2>Source claim</h2><div class="body"><p class="dim">No source has been claimed for this entry. That is the honest default — resolution and metadata do not depend on a claim.</p>{rebuild}</div></div>"#,
            rebuild = rebuild_block(site, e),
        ),
        Some(c) => {
            let repo_dd = if c.repository.starts_with("http://")
                || c.repository.starts_with("https://")
            {
                format!(
                    r#"<a href="{r}" rel="noopener noreferrer nofollow" target="_blank">{r}</a>"#,
                    r = esc(&c.repository)
                )
            } else {
                esc(&c.repository)
            };
            format!(
                r#"<div class="panel"><h2>Source claim</h2><div class="body"><dl class="kv">
  <div><dt>Repository</dt><dd>{repo}</dd></div>
  <div><dt>Commit</dt><dd class="mono">{commit}{commit_copy}</dd></div>
  <div><dt>Source digest</dt><dd class="mono">{sdigest}{sdigest_copy}</dd></div>
  <div><dt>Toolchain</dt><dd class="mono">{toolchain}</dd></div>
  <div><dt>Builder image</dt><dd class="mono">{image}</dd></div>
  <div><dt>Offline build</dt><dd>{offline}</dd></div>
</dl>
<p class="dim" style="margin:14px 0 0">A repository is a hint, not a dependency: the <b>source digest</b> is a hash over the tree as built, so any mirror that reproduces it is as good as the original host.</p>
{rebuild}</div></div>"#,
                repo = repo_dd,
                commit = esc(&hex(&c.commit)),
                commit_copy = copy_btn(&hex(&c.commit)),
                sdigest = esc(&hex(&c.source_digest)),
                sdigest_copy = copy_btn(&hex(&c.source_digest)),
                toolchain = esc(&c.build.toolchain),
                image = esc(&short_hex(&c.build.image_digest)),
                offline = if c.build.network_disabled {
                    r#"<span class="mono">yes</span> — network was disabled, which is what makes a match mean anything"#
                } else {
                    r#"<span class="mono">no</span> — the build could have fetched the artifact it was meant to reproduce"#
                },
                rebuild = rebuild_block(site, e),
            )
        }
    };

    let disclaimer = r#"<div class="disclaimer"><span class="mk">!</span><p><b>Verified does not mean safe.</b> It proves one thing: rebuilding the stated source, at the stated commit, with the stated toolchain, produced the deployed bytecode. It does not prove the program is safe, that the author is who they say, or that the code does what its README claims.</p></div>"#;

    let body = format!(
        r#"<a class="back" href="/">← All programs</a>
<div class="detail-head"><h1>{name}</h1><span class="ver">{ver}</span>{publisher}</div>
<p class="detail-sub">{desc}</p>
{state_callout}
{disclaimer}
<div class="grid2" style="margin-top:22px">
  <div>{meta}{idl}</div>
  <div>{trace}{source}</div>
</div>"#,
        name = esc(&e.name),
        ver = esc(&e.version),
        publisher = publisher_tag(&e.publisher),
        desc = if e.description.is_empty() {
            r#"<span class="faint">No description.</span>"#.to_string()
        } else {
            esc(&e.description)
        },
        state_callout = state_callout,
        disclaimer = disclaimer,
        meta = meta,
        idl = idl_panel(idl),
        trace = derivation_trace(site, e, publisher),
        source = source_panel,
    );

    // Social card: title carries the verification state (the thing worth seeing
    // before a click), description is the plain-English verdict plus whatever the
    // author described. Canonical points at this entry's own permalink, carrying
    // the publisher for a third-party entry so the card resolves back to itself.
    let publisher_word = match e.publisher {
        Publisher::Canonical => "canonical",
        Publisher::ThirdParty => "third-party",
    };
    let card_desc = clip(
        &format!(
            "{label} · {publisher_word}. {tail}",
            label = f.label,
            tail = if e.description.is_empty() {
                f.sentence
            } else {
                &e.description
            },
        ),
        200,
    );
    let card = Meta {
        description: card_desc,
        og_title: format!("{} {} — {}", e.name, e.version, f.label),
        canonical_path: detail_href(e),
    };
    page_meta(
        site,
        &format!("{} {} · LEZ Registry Explorer", e.name, e.version),
        "",
        &card,
        &body,
    )
}

/// Plain-text clip to a character budget, for a meta description. Not HTML —
/// `esc` runs later when it is placed into an attribute.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut o: String = s.chars().take(max.saturating_sub(1)).collect();
        o.push('…');
        o
    }
}

/// How a reader rebuilds this exact entry and checks it locally. The whole
/// registry's argument is that this does not need the site.
fn rebuild_block(_site: &Site, e: &Entry) -> String {
    let pid = hex(&e.program_id);
    let claimed = e.verification.claim.is_some();
    let rebuild = if claimed {
        format!(
            r#"

<span class="c"># Rebuild the claimed source sealed (network disabled) and compare the ImageID:</span>
./scripts/build-sealed.sh
spel program-id artifacts/programs/*.bin   <span class="c"># must equal the program id below</span>
lez-registry deployment-digest artifacts/programs/*.bin

<span class="c"># expected program id</span>
<b>{pid}</b>"#,
            pid = esc(&pid)
        )
    } else {
        String::new()
    };
    format!(
        r#"<div style="margin-top:14px"><span class="eyebrow">Rebuild &amp; verify locally</span>
<pre class="code"><span class="c"># Resolve this entry yourself — one hash, one account read, no index:</span>
lez-registry lookup <b>{pid}</b>{rebuild}</pre></div>"#,
        pid = esc(&pid),
        rebuild = rebuild,
    )
}

// ---------------------------------------------------------------------------
// notices
// ---------------------------------------------------------------------------

pub fn not_found(site: &Site, heading: &str, detail_html: &str) -> String {
    let body = format!(
        r#"<div class="notice"><h2>{h}</h2><div class="dim">{d}</div></div>"#,
        h = esc(heading),
        d = detail_html,
    );
    page(
        site,
        &format!("{heading} · LEZ Registry Explorer"),
        "",
        &body,
    )
}

// ---------------------------------------------------------------------------
// about — "How this stays decentralised", stated plainly, plus the explorer
// verifying itself. Nothing here is boilerplate: each principle links to the
// mechanism that makes it true, so a reader can check the claim, not just read it.
// ---------------------------------------------------------------------------

pub fn about_page(site: &Site, stats: Option<(usize, u64, u64)>) -> String {
    // Live on-chain numbers when the chain answered; a plain note when it did not
    // — the argument does not depend on a successful walk.
    let stats_html = match stats {
        Some((walked, verified, total)) => format!(
            r#"<div class="stats" style="margin:18px 0 26px">
  <div class="stat"><span class="n">{walked}</span><span class="l">Entries walked</span></div>
  <div class="stat accent"><span class="n">{verified}</span><span class="l">Verified</span></div>
  <div class="stat"><span class="n">{total}</span><span class="l">On-chain counter</span></div>
</div>"#
        ),
        None => r#"<p class="detail-sub">Live counts are unavailable right now — the sequencer did not answer. That is itself the point: the registry does not depend on this site, and neither does anything below.</p>"#.to_string(),
    };

    // The four principles. Each states the promise and then the mechanism that
    // keeps it, and links to where a reader can see that mechanism at work.
    let principles = format!(
        r#"<div class="principles">
  <div class="principle">
    <span class="pn">01</span>
    <h3>Read-only. No proprietary database.</h3>
    <p>Every value on every page is a live read of the chain through the same <span class="mono">lez-registry-sdk</span> the CLI links. The only state held is an in-memory walk cache with a short TTL, rebuilt from the chain and discarded on restart; nothing resolves <em>from</em> it. Run with <span class="mono">--no-cache</span> to remove even that. There is no table only this site has.</p>
  </div>
  <div class="principle">
    <span class="pn">02</span>
    <h3>Open source, self-hostable in one command — same data for everyone.</h3>
    <p>The whole install is one command. Two instances pointed at the same registry and endpoint render the same bytes, because the data was never the server's — it belongs to the chain.</p>
    <pre class="code"><span class="c"># Docker — pinned base, sealed offline build, nothing else to set up</span>
docker build -t lez-registry-explorer -f explorer/Dockerfile .
docker run --rm -p 8080:8080 lez-registry-explorer

<span class="c"># or straight from source</span>
cargo run --release --manifest-path explorer/Cargo.toml</pre>
  </div>
  <div class="principle">
    <span class="pn">03</span>
    <h3>The search index is the same open-source, self-hostable indexer.</h3>
    <p>Search here runs the identical filter over the identical walk the repo's open-source <span class="mono">indexer</span> exposes as a service — no private index, no hosted secret. Run the indexer yourself when you want search to be a separate process; neither it nor this site is ever required to <em>resolve</em> a program. Resolution is a hash and one account read.</p>
  </div>
  <div class="principle">
    <span class="pn">04</span>
    <h3>The registry works fully without this site.</h3>
    <p>The module, CLI and SDK are primary; the explorer is an accelerator for browsing, never a dependency for resolving. Turn it off and <span class="mono">lez-registry lookup &lt;id&gt;</span> still answers. Every entry page shows the <a href="{reg_entry}">derivation</a> that computes the account from the program id, ending in the exact <span class="mono">getAccount</span> you could run instead of trusting this page.</p>
  </div>
</div>"#,
        reg_entry = esc(&format!("/program/{}", site.registry_hex)),
    );

    let self_verify = self_verify_section(site);

    let states = format!(
        r#"<section class="panel" style="margin-top:26px"><h2>The five verification states, and one caveat</h2><div class="body">
  <p class="dim" style="margin:0 0 12px">Verified is the only green in this explorer, and it is earned rather than assumed. None of the five is a tick — each says exactly what is known and no more.</p>
  {legend}
  <div class="disclaimer" style="margin-top:18px"><span class="mk">!</span><p><b>Verified does not mean safe.</b> It proves one thing: rebuilding the stated source, at the stated commit, with the stated toolchain, produced the deployed bytecode. It does not prove the program is safe, that the author is who they say, or that the code does what its README claims.</p></div>
</div></section>"#,
        legend = legend_grid_only(),
    );

    let body = format!(
        r#"<span class="eyebrow">About</span>
<h1 class="page-title">How this stays decentralised</h1>
<p class="detail-sub">A block explorer is the classic place a "no mandatory external services" promise quietly dies — the "official" explorer becomes the thing everyone actually trusts. This one is built so it cannot. Four principles, each checkable rather than merely asserted.</p>
{stats}
{principles}
{self_verify}
{states}"#,
        stats = stats_html,
        principles = principles,
        self_verify = self_verify,
        states = states,
    );

    let meta = Meta {
        description: "How the LEZ Registry Explorer stays decentralised: read-only, no database, one-command self-host, an open-source search index, and a registry that works without the site — plus the explorer verifying its own build.".to_string(),
        og_title: "How the LEZ Registry Explorer stays decentralised".to_string(),
        canonical_path: "/about".to_string(),
    };
    page_meta(site, "About · LEZ Registry Explorer", "", &meta, &body)
}

/// The explorer verifying itself: its own repo, the commit the running binary was
/// built from, the sha256 of that binary, and the exact commands to rebuild it
/// and confirm the running site byte-for-byte.
fn self_verify_section(site: &Site) -> String {
    let commit_is_pinned =
        site.commit.len() >= 40 && site.commit.chars().all(|c| c.is_ascii_hexdigit());
    let pin_note = if commit_is_pinned {
        String::new()
    } else {
        r#"<p class="dim" style="margin:10px 0 0"><b>This binary was not built from the pinned image</b>, so its commit is not stamped and its digest is machine-specific. Build it with <span class="mono">explorer/Dockerfile</span> for a commit and a digest every rebuilder reproduces.</p>"#.to_string()
    };
    format!(
        r#"<section id="self-verify" class="panel self-verify-panel" style="margin-top:26px"><h2>The explorer verifies itself</h2><div class="body">
  <p class="dim" style="margin:0 0 14px">This registry's whole argument is that you should be able to rebuild what is running and confirm it, rather than trust a badge. The explorer holds itself to that: below is its own source, the commit this binary was built from, and the sha256 of the binary now serving you. Rebuild the same commit reproducibly and the digest matches.</p>
  <dl class="kv">
    <div><dt>Source repository</dt><dd><a href="{repo}" rel="noopener noreferrer" target="_blank">{repo}</a></dd></div>
    <div><dt>Built from commit</dt><dd class="mono">{commit}{commit_copy}</dd></div>
    <div><dt>Running binary</dt><dd class="mono">sha256 {digest}{digest_copy}</dd></div>
  </dl>
  {pin_note}
  <p class="dim" style="margin:16px 0 8px">Reproduce it yourself:</p>
  <pre class="code"><span class="c"># 1. Rebuild the explorer at the commit above, sealed and reproducible</span>
git clone {repo}
cd lp-0023-program-registry &amp;&amp; git checkout {commit_arg}
docker build -t lez-registry-explorer -f explorer/Dockerfile .

<span class="c"># 2. Print the binary's sha256 and compare with the running one above</span>
docker run --rm --entrypoint sha256sum lez-registry-explorer \
  /usr/local/bin/lez-registry-explorer
<span class="c"># expected</span>
<b>{digest}</b></pre>
  <p class="dim" style="margin:14px 0 0">Registered in the registry it displays? The registry keys on RISC0 ImageIDs — on-chain programs — and this explorer is a native binary, so it is honestly <em>not</em> an on-chain entry. What it can do, and does here, is practise the same source-verification discipline it exposes: a named source, a pinned toolchain, a sealed offline build, and a published digest anyone can reproduce. The registry's own program does the on-chain version of exactly this — see <a href="{reg_entry}">its self-attested entry</a>, slot 0, the registry registered in itself.</p>
</div></section>"#,
        repo = esc(site.repo),
        commit = esc(site.commit),
        commit_copy = if commit_is_pinned {
            copy_btn(site.commit)
        } else {
            String::new()
        },
        commit_arg = esc(if commit_is_pinned {
            site.commit
        } else {
            "HEAD"
        }),
        digest = esc(site.self_digest),
        digest_copy = copy_btn(site.self_digest),
        pin_note = pin_note,
        reg_entry = esc(&format!("/program/{}", site.registry_hex)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The content-id encoder is the one piece the IDL panel relies on that no
    /// live entry currently exercises (none has an IDL pointer on chain). Pin it
    /// against the two identifiers the core crate documents from a real Storage
    /// node: decode with the core's own decoder, re-encode here, and require the
    /// exact string back. A drift here would render a valid-looking `z…` that
    /// fetches the wrong object.
    #[test]
    fn content_id_string_round_trips_known_identifiers() {
        for cid in [
            "zDvZRwzm9Ni32iyvoy868YVrBHFz8vo3oZirrCKhFMMn9aXuinfQ",
            "zDzSvJTf49uAN2BZhoMhZmDVSv7KDVeUVXTXvDc6QNnU1TKJYD55",
        ] {
            let bytes = core_::decode_content_id(cid).expect("known-good CID decodes");
            assert_eq!(
                content_id_string(&bytes),
                cid,
                "re-encoding must reproduce {cid}"
            );
        }
    }

    #[test]
    fn fmt_millis_is_utc_civil() {
        assert_eq!(fmt_millis(0), "1970-01-01 00:00 UTC");
        // Unix second 1_000_000_000 = 2001-09-09 01:46:40 UTC.
        assert_eq!(fmt_millis(1_000_000_000_000), "2001-09-09 01:46 UTC");
    }

    #[test]
    fn a_third_party_detail_link_carries_the_publisher() {
        let author: AccountId = [9u8; 32];
        let mut e = Entry::new(
            [3u8; 32],
            Stamp {
                block: 1,
                millis: 0,
            },
            "x".into(),
            "1".into(),
            author,
            Publisher::ThirdParty,
        )
        .unwrap();
        e.publisher = Publisher::ThirdParty;
        let href = detail_href(&e);
        assert!(
            href.contains("?publisher="),
            "third-party link must resolve back to itself, not the canonical entry"
        );
    }

    /// A verified, self-attested entry, used to check the social card reflects the
    /// verification state rather than a generic title.
    fn verified_entry() -> Entry {
        let author: AccountId = [1u8; 32];
        let mut e = Entry::new(
            [7u8; 32],
            Stamp {
                block: 1,
                millis: 0,
            },
            "widget".into(),
            "1.2".into(),
            author,
            Publisher::Canonical,
        )
        .unwrap();
        e.verification.state = VerificationState::Verified;
        e.verification.attester = Some(author);
        e
    }

    #[test]
    fn a_permalink_carries_open_graph_and_an_absolute_canonical_when_the_origin_is_known() {
        let registry = [0u8; 32];
        let registry_hex = hex(&registry);
        let site = Site {
            registry,
            registry_hex: &registry_hex,
            endpoint: "https://testnet.lez.logos.co",
            origin: Some("https://x.example".into()),
            repo: "https://example/repo",
            commit: "abc",
            self_digest: "deadbeef",
        };
        let e = verified_entry();
        let html = detail_page(&site, &e, None, &IdlView::None);
        assert!(html.contains("og:title"), "an og:title is emitted");
        assert!(
            html.contains(r#"name="twitter:card" content="summary""#),
            "a twitter summary card is declared"
        );
        // The card title reflects name, version and the verification state, so a
        // shared link says what is known before it is clicked.
        assert!(
            html.contains("widget 1.2"),
            "og:title carries name and version"
        );
        assert!(
            html.contains("Verified"),
            "og:title carries the verification state"
        );
        assert!(
            html.contains(r#"<link rel="canonical" href="https://x.example/program/"#),
            "canonical is absolute, built from the request origin"
        );
        assert!(
            html.contains(r#"property="og:url" content="https://x.example/program/"#),
            "og:url is absolute, built from the request origin"
        );
    }

    #[test]
    fn a_permalink_omits_absolute_urls_when_the_origin_is_unknown() {
        let registry = [0u8; 32];
        let registry_hex = hex(&registry);
        let site = Site {
            registry,
            registry_hex: &registry_hex,
            endpoint: "https://testnet.lez.logos.co",
            origin: None,
            repo: "https://example/repo",
            commit: "abc",
            self_digest: "deadbeef",
        };
        let e = verified_entry();
        let html = detail_page(&site, &e, None, &IdlView::None);
        assert!(
            !html.contains(r#"rel="canonical""#),
            "no canonical is invented without a known origin"
        );
        assert!(
            !html.contains("og:url"),
            "no og:url is invented without an origin"
        );
        assert!(
            html.contains("og:title"),
            "the card title is still emitted; only the absolute-URL tags are held back"
        );
    }

    #[test]
    fn about_states_the_four_principles_the_caveat_and_the_running_binary_digest() {
        let registry = [0u8; 32];
        let registry_hex = hex(&registry);
        let site = Site {
            registry,
            registry_hex: &registry_hex,
            endpoint: "https://testnet.lez.logos.co",
            origin: Some("https://x.example".into()),
            repo: "https://example/repo",
            commit: "0123456789abcdef0123456789abcdef01234567",
            self_digest: "cafebabecafebabe",
        };
        let html = about_page(&site, Some((6, 1, 6)));
        assert!(html.contains("How this stays decentralised"));
        assert_eq!(
            html.matches(r#"class="principle">"#).count(),
            4,
            "exactly four principles are stated"
        );
        assert!(
            html.contains("cafebabecafebabe"),
            "the running binary's sha256 is shown so a rebuild can be compared"
        );
        assert!(
            html.contains("0123456789abcdef"),
            "the commit the binary was built from is shown"
        );
        assert!(
            html.contains("Verified does not mean safe"),
            "the safety caveat appears where states are explained"
        );
    }
}

// ---------------------------------------------------------------------------
// adoption
// ---------------------------------------------------------------------------

fn meter(label: &str, have: usize, want: usize, note: &str) -> String {
    let pct = if want == 0 { 100 } else { (have * 100 / want).min(100) };
    let done = have >= want;
    format!(
        r#"<div class="kpi{cls}">
  <div class="kpi-head"><span class="kpi-label">{label}</span><span class="kpi-num">{have}<span class="faint"> / {want}</span></span></div>
  <div class="kpi-bar" role="progressbar" aria-valuenow="{have}" aria-valuemin="0" aria-valuemax="{want}"><span style="width:{pct}%"></span></div>
  <div class="kpi-note">{note}</div>
</div>"#,
        cls = if done { " met" } else { "" },
        label = esc(label),
        have = have,
        want = want,
        pct = pct,
        note = note,
    )
}

pub fn adoption_page(site: &Site, a: &crate::adoption::Adoption, team: &[AccountId]) -> String {
    use crate::adoption::*;
    let months = if a.per_month.is_empty() {
        r#"<p class="dim">No dated registrations yet.</p>"#.to_string()
    } else {
        let max = a.per_month.values().copied().max().unwrap_or(1).max(TARGET_PER_MONTH);
        let bars: String = a
            .per_month
            .iter()
            .map(|(&(y, m), &n)| {
                format!(
                    r#"<div class="month{cls}"><span class="month-bar" style="height:{h}%"><b class="month-n">{n}</b></span><span class="month-l">{y}-{m:02}</span></div>"#,
                    cls = if n >= TARGET_PER_MONTH { " met" } else { "" },
                    h = (n * 100 / max).max(2),
                )
            })
            .collect();
        format!(
            r#"<div class="months" style="--line:{line}%">{bars}<span class="months-line" title="{t} a month"></span></div>"#,
            line = TARGET_PER_MONTH * 100 / max,
            t = TARGET_PER_MONTH,
        )
    };
    let team_line = if team.is_empty() {
        "No team accounts declared on this instance, so every author counts as independent. The operator passes them with <span class=\"mono\">--team</span>.".to_string()
    } else {
        format!(
            "Excluded as the submitting team: {}.",
            team.iter()
                .map(|t| format!("<span class=\"mono\">{}</span>", esc(&short_b58(&sdk_b58(t)))))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let body = format!(
        r#"<span class="eyebrow">LP-0023 adoption, counted from the chain</span>
<h1 class="page-title">Adoption</h1>
<p class="detail-sub">Every figure below is computed on this page load from the entries the registry holds and the sequencer's timestamp on each. Nothing here is typed in by a maintainer, and a second instance pointed at the same registry shows the same numbers.</p>
<p class="adoption-verdict{vcls}">{verdict}</p>
<div class="kpis">
{programs}
{authors}
{verified}
{run}
</div>
<section class="panel adoption-months">
  <h2 class="panel-title">Registrations per month (UTC)</h2>
  <p class="dim">The criterion is sustained activity: at least {per} new registrations in each of at least {mn} consecutive months. The line marks {per}.</p>
  {months}
  {undated}
</section>
<section class="panel">
  <h2 class="panel-title">Not decidable from the chain</h2>
  <ul class="plain">
    <li><strong>5 third-party Logos UI Modules</strong> integrating the SDK, independent of the team and of each other.</li>
    <li><strong>20 Discord and 20 Twitter/X testimonials</strong> describing what the person used the registry for.</li>
  </ul>
  <p class="dim">These are gathered and linked in the submission, not counted here. {team}</p>
</section>"#,
        programs = meter("Registered programs", a.programs, TARGET_PROGRAMS, "distinct program ids with an entry"),
        authors = meter("Independent authors", a.independent_authors, TARGET_AUTHORS, &format!("{} distinct author{} in all", a.authors, if a.authors == 1 { "" } else { "s" })),
        verified = meter(
            "Verified source claims",
            a.verified_independent + a.verified_self,
            TARGET_VERIFIED,
            &format!("{} rebuilt by another party, {} by their own author; {} entries carry a claim", a.verified_independent, a.verified_self, a.with_claim),
        ),
        run = meter("Consecutive months at pace", a.best_run, TARGET_MONTHS, &format!("months with at least {} registrations", TARGET_PER_MONTH)),
        per = TARGET_PER_MONTH,
        mn = TARGET_MONTHS,
        months = months,
        undated = if a.undated > 0 { format!("<p class=\"faint\">{} entries carry no timestamp and are not counted in a month.</p>", a.undated) } else { String::new() },
        team = team_line,
        vcls = if a.on_chain_met() { " met" } else { "" },
        verdict = if a.on_chain_met() {
            "Every on-chain adoption criterion is met.".to_string()
        } else {
            let met = [a.programs >= TARGET_PROGRAMS, a.independent_authors >= TARGET_AUTHORS,
                       a.verified_independent + a.verified_self >= TARGET_VERIFIED, a.best_run >= TARGET_MONTHS]
                .iter().filter(|x| **x).count();
            format!("{met} of 4 on-chain adoption criteria met.")
        },
    );
    page(site, "Adoption · LEZ Registry Explorer", "", &body)
}

fn sdk_b58(a: &AccountId) -> String {
    base58btc(a)
}
