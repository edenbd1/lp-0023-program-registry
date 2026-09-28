//! Adoption, counted from chain data only.
//!
//! LP-0023 is won on traction as much as on code: fifty registered programs from
//! at least twenty independent authors, twenty of them with a source claim an
//! evaluator can rebuild, spread over at least two months with at least ten new
//! registrations in each. Every one of those is decidable from what the chain
//! holds — the entries and the sequencer-stamped `registered_at` on each — so
//! the explorer computes them rather than a maintainer asserting them.
//!
//! "Independent of the submitting team" is the one input the chain cannot
//! supply. The operator passes the team's accounts (`--team`, repeatable) and
//! they are excluded from the author count; the page says which accounts were
//! excluded, so the exclusion itself is inspectable.

use lez_registry_core::{AccountId, Entry, VerificationState};
use std::collections::{BTreeMap, BTreeSet};

/// The prize's thresholds, as published in `prizes/LP-0023.md`.
pub const TARGET_PROGRAMS: usize = 50;
pub const TARGET_AUTHORS: usize = 20;
pub const TARGET_VERIFIED: usize = 20;
pub const TARGET_MONTHS: usize = 2;
pub const TARGET_PER_MONTH: usize = 10;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Adoption {
    /// Distinct program ids with at least one entry.
    pub programs: usize,
    /// Distinct authors across every entry.
    pub authors: usize,
    /// Distinct authors outside the declared team.
    pub independent_authors: usize,
    /// Entries whose source claim an independent party rebuilt to a match.
    pub verified_independent: usize,
    /// Entries verified only by their own author.
    pub verified_self: usize,
    /// Entries carrying a source claim, whatever its state.
    pub with_claim: usize,
    /// Registrations per UTC calendar month, `(year, month)` → count.
    pub per_month: BTreeMap<(i64, u32), usize>,
    /// The longest run of consecutive months each at or above the threshold.
    pub best_run: usize,
    /// Entries with no usable timestamp (a zero clock reading).
    pub undated: usize,
}

impl Adoption {
    pub fn from_entries(entries: &[Entry], team: &[AccountId]) -> Self {
        let team: BTreeSet<&AccountId> = team.iter().collect();
        let programs: BTreeSet<_> = entries.iter().map(|e| e.program_id).collect();
        let authors: BTreeSet<_> = entries.iter().map(|e| e.author).collect();
        let independent = authors.iter().filter(|a| !team.contains(a)).count();
        let mut per_month = BTreeMap::new();
        let mut undated = 0;
        for e in entries {
            if e.registered_at.millis == 0 {
                undated += 1;
                continue;
            }
            *per_month.entry(year_month(e.registered_at.millis)).or_insert(0) += 1;
        }
        let verified_self = entries
            .iter()
            .filter(|e| e.verification.is_self_attested(&e.author))
            .count();
        let verified_all = entries
            .iter()
            .filter(|e| e.verification.state == VerificationState::Verified)
            .count();
        Self {
            programs: programs.len(),
            authors: authors.len(),
            independent_authors: independent,
            verified_independent: verified_all - verified_self,
            verified_self,
            with_claim: entries.iter().filter(|e| e.verification.claim.is_some()).count(),
            best_run: best_run(&per_month, TARGET_PER_MONTH),
            per_month,
            undated,
        }
    }

    /// Every on-chain criterion met at once.
    pub fn on_chain_met(&self) -> bool {
        self.programs >= TARGET_PROGRAMS
            && self.independent_authors >= TARGET_AUTHORS
            && self.verified_independent + self.verified_self >= TARGET_VERIFIED
            && self.best_run >= TARGET_MONTHS
    }
}

/// UTC `(year, month)` of an epoch-milliseconds instant (Hinnant's
/// `civil_from_days`, the same arithmetic the page's timestamps use).
pub fn year_month(ms: u64) -> (i64, u32) {
    let days = ((ms / 1000) as i64).div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32)
}

/// Longest run of *consecutive* calendar months each with at least `min`
/// registrations. A gap month breaks the run, because the criterion is about
/// sustained activity rather than two good months a year apart.
pub fn best_run(per_month: &BTreeMap<(i64, u32), usize>, min: usize) -> usize {
    let (mut best, mut run) = (0usize, 0usize);
    let mut prev: Option<(i64, u32)> = None;
    for (&ym, &n) in per_month {
        let consecutive = prev.is_some_and(|p| next_month(p) == ym);
        if n >= min {
            run = if consecutive { run + 1 } else { 1 };
            best = best.max(run);
        } else {
            run = 0;
        }
        prev = Some(ym);
    }
    best
}

fn next_month((y, m): (i64, u32)) -> (i64, u32) {
    if m == 12 { (y + 1, 1) } else { (y, m + 1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400_000;
    // 2026-09-01T00:00:00Z
    const SEP1: u64 = 1_788_220_800_000;

    #[test]
    fn months_are_utc_calendar_months() {
        assert_eq!(year_month(SEP1), (2026, 9));
        assert_eq!(year_month(SEP1 - 1), (2026, 8), "the last millisecond of August");
        assert_eq!(year_month(SEP1 + 30 * DAY), (2026, 10));
        assert_eq!(year_month(0), (1970, 1));
    }

    #[test]
    fn a_run_needs_consecutive_months_each_over_the_threshold() {
        let m = |v: &[((i64, u32), usize)]| v.iter().cloned().collect::<BTreeMap<_, _>>();
        assert_eq!(best_run(&m(&[((2026, 10), 12), ((2026, 11), 10)]), 10), 2);
        assert_eq!(best_run(&m(&[((2026, 10), 12), ((2026, 11), 9)]), 10), 1, "one month short");
        assert_eq!(best_run(&m(&[((2026, 10), 30), ((2026, 12), 30)]), 10), 1, "a gap breaks the run");
        assert_eq!(best_run(&m(&[((2026, 12), 10), ((2027, 1), 10), ((2027, 2), 10)]), 10), 3, "across a year boundary");
        assert_eq!(best_run(&BTreeMap::new(), 10), 0);
    }
}
