//! "Locally verified": a mark a user keeps for themselves.
//!
//! The prize lets a user rebuild an entry's recorded source and, when the
//! bytecode matches, mark the program as locally verified *for themselves*.
//! That mark is a statement about this machine's rebuild, not a claim the
//! registry can hold on anyone's behalf, so it lives here — in a plain file in
//! the user's config directory — and never touches the chain.
//!
//! One line per mark, tab-separated, readable without this tool:
//!
//! ```text
//! <program id hex>  <deployment digest hex>  <sha256 of the rebuilt file>  <unix seconds>
//! ```
//!
//! The location is `$LEZ_REGISTRY_HOME/locally-verified.tsv`, else
//! `$XDG_CONFIG_HOME/lez-registry/…`, else `~/.config/lez-registry/…`.

use std::io::Write as _;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub program_id: String,
    pub digest: String,
    pub file_sha256: String,
    pub marked_at: u64,
}

pub fn store_path() -> PathBuf {
    if let Some(h) = std::env::var_os("LEZ_REGISTRY_HOME") {
        return PathBuf::from(h).join("locally-verified.tsv");
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("lez-registry").join("locally-verified.tsv")
}

pub fn load() -> Vec<Mark> {
    let Ok(text) = std::fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            match f.as_slice() {
                [p, d, s, t] => Some(Mark {
                    program_id: p.to_string(),
                    digest: d.to_string(),
                    file_sha256: s.to_string(),
                    marked_at: t.parse().ok()?,
                }),
                // A line this tool did not write is skipped, not fatal: the
                // file is the user's, and they may annotate it.
                _ => None,
            }
        })
        .collect()
}

/// The most recent mark for `program_id`, if any.
pub fn mark_for(program_id: &str) -> Option<Mark> {
    load()
        .into_iter()
        .filter(|m| m.program_id == program_id)
        .max_by_key(|m| m.marked_at)
}

pub fn add(m: &Mark) -> std::io::Result<PathBuf> {
    let path = store_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(
        f,
        "{}\t{}\t{}\t{}",
        m.program_id, m.digest, m.file_sha256, m.marked_at
    )?;
    Ok(path)
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_round_trips_and_the_latest_one_wins() {
        let dir = std::env::temp_dir().join(format!("lez-registry-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("LEZ_REGISTRY_HOME", &dir);
        assert_eq!(mark_for("aa"), None, "nothing is marked on a fresh machine");
        let older = Mark {
            program_id: "aa".into(),
            digest: "d1".into(),
            file_sha256: "s1".into(),
            marked_at: 10,
        };
        let newer = Mark {
            marked_at: 20,
            digest: "d2".into(),
            ..older.clone()
        };
        add(&older).unwrap();
        add(&newer).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(store_path())
            .unwrap()
            .write_all(b"# a note the user added\n")
            .unwrap();
        assert_eq!(mark_for("aa"), Some(newer));
        assert_eq!(mark_for("bb"), None, "a mark is per program");
        assert_eq!(load().len(), 2, "an unparseable line is skipped, not fatal");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
