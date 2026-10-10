//! Core logic for `sessionize`: assign a session id to keyed, timestamped events.
//! Kept free of I/O so it can be unit-tested without spawning the binary.
//!
//! Facts, not a verdict: it reproduces a session definition (idle gap), it does not
//! claim the gap is the *right* boundary, and it does not prevent reopen-merge —
//! events within `gap` of the previous stay in one session, merges and all. Whether
//! that definition distorts a downstream metric is for the downstream to show.

use chrono::{DateTime, Duration, FixedOffset};

/// Assign a session id to each event, returned **aligned to the input order**.
///
/// Events are grouped per key in time order (sorted internally — the input need not
/// be sorted). Within a key, an idle gap strictly greater than `gap` starts a new
/// session, as does a new key. Ids are numbered in (key, time) order but returned in
/// the caller's order, so rows can be emitted unchanged.
///
/// `gap` must be non-negative (the CLI guarantees this). With a non-negative gap,
/// same-timestamp events never land in different sessions regardless of order (a zero
/// gap compares equal, not greater); a negative gap would split even equal timestamps.
pub fn sessionize(events: &[(String, DateTime<FixedOffset>)], gap: Duration) -> Vec<u64> {
    debug_assert!(gap >= Duration::zero(), "gap must be non-negative");
    let mut order: Vec<usize> = (0..events.len()).collect();
    // Unstable is fine: ties (same key+time) share a session (a zero gap never
    // splits), so their order can't change the result, and output order is restored
    // from the input separately.
    order.sort_unstable_by(|&a, &b| {
        events[a]
            .0
            .cmp(&events[b].0)
            .then(events[a].1.cmp(&events[b].1))
    });

    let mut ids = vec![0u64; events.len()];
    let mut sid = 0u64;
    let mut cur_key: Option<&str> = None;
    let mut prev: Option<DateTime<FixedOffset>> = None;
    for &i in &order {
        let (key, time) = (events[i].0.as_str(), events[i].1);
        let new_key = cur_key != Some(key);
        let gapped = !new_key && prev.is_some_and(|p| time - p > gap);
        if new_key || gapped {
            sid += 1;
            cur_key = Some(key);
        }
        prev = Some(time);
        ids[i] = sid;
    }
    ids
}

/// The number of distinct sessions in an id vector from [`sessionize`].
pub fn session_count(ids: &[u64]) -> u64 {
    ids.iter().copied().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn ev(k: &str, ts: &str) -> (String, DateTime<FixedOffset>) {
        (k.to_string(), t(ts))
    }
    const GAP: Duration = Duration::minutes(10);

    #[test]
    fn under_gap_is_one_session() {
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:09:00+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(session_count(&ids), 1);
    }

    #[test]
    fn over_gap_splits() {
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:11:00+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn exactly_gap_stays_together() {
        // diff == gap, split is strictly greater — so NOT split.
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:10:00+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        assert_eq!(ids[0], ids[1]);
    }

    #[test]
    fn keys_never_mix() {
        // interleaved in time, but u1 and u2 must be separate sessions.
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u2", "2026-10-09T12:00:30+09:00"),
            ev("u1", "2026-10-09T12:00:40+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        assert_eq!(ids[0], ids[2]); // both u1
        assert_ne!(ids[0], ids[1]); // u2 is its own
        assert_eq!(session_count(&ids), 2);
    }

    #[test]
    fn reopen_within_gap_stays_merged() {
        // open, cat, cat, re-open (10m later, within gap), cat — one merged session.
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:00:02+09:00"),
            ev("u1", "2026-10-09T12:00:05+09:00"),
            ev("u1", "2026-10-09T12:10:00+09:00"),
            ev("u1", "2026-10-09T12:10:02+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        assert_eq!(session_count(&ids), 1);
    }

    #[test]
    fn unsorted_input_ids_align_to_input_order() {
        let e = [
            ev("u1", "2026-10-09T12:20:00+09:00"), // latest
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:05:00+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        // 12:00 & 12:05 (5m) share a session; 12:20 (15m later) is separate.
        assert_eq!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn same_timestamp_never_splits() {
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T12:00:03+09:00"),
        ];
        let ids = sessionize(&e, GAP);
        assert_eq!(session_count(&ids), 1);
    }

    #[test]
    fn same_instant_different_offset_is_one_session() {
        // 12:00+09:00 == 03:00Z — compared on the instant, not the wall-clock text.
        let e = [
            ev("u1", "2026-10-09T12:00:00+09:00"),
            ev("u1", "2026-10-09T03:00:00Z"),
        ];
        let ids = sessionize(&e, GAP);
        assert_eq!(session_count(&ids), 1);
    }

    #[test]
    fn empty_is_empty() {
        assert_eq!(sessionize(&[], GAP), Vec::<u64>::new());
        assert_eq!(session_count(&[]), 0);
    }
}
