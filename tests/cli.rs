//! End-to-end smoke tests: run the built binary with args and stdin. std only.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_data-probe"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn data-probe");
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().expect("wait data-probe")
}

const HEADER: &str = "user_id\ttimestamp\tevent";

fn sessions(stdin: &str, gap: &str) -> Output {
    run(
        &[
            "sessions",
            "--key",
            "user_id",
            "--time",
            "timestamp",
            "--gap",
            gap,
        ],
        stdin,
    )
}

// session_id column of each data row, in output order.
fn ids(out: &Output) -> Vec<String> {
    let s = String::from_utf8(out.stdout.clone()).unwrap();
    s.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.rsplit('\t').next().unwrap().to_string())
        .collect()
}

#[test]
fn reopen_within_gap_stays_one_session_and_output_keeps_input_order() {
    let input = format!(
        "{HEADER}\n\
         u1\t2026-10-09T12:00:00+09:00\topen\n\
         u1\t2026-10-09T12:00:05+09:00\tcategory\n\
         u1\t2026-10-09T12:10:00+09:00\topen\n"
    );
    let out = sessions(&input, "30m");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    let first = stdout.lines().next().unwrap();
    assert_eq!(first, format!("{HEADER}\tsession_id"));
    let id = ids(&out);
    assert_eq!(id, vec!["1", "1", "1"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("sessions 1"), "{stderr}");
}

#[test]
fn idle_gap_splits() {
    let input = format!(
        "{HEADER}\n\
         u1\t2026-10-09T12:00:00+09:00\topen\n\
         u1\t2026-10-09T12:40:00+09:00\topen\n"
    );
    let out = sessions(&input, "30m");
    assert!(out.status.success());
    assert_eq!(ids(&out), vec!["1", "2"]);
}

#[test]
fn unsorted_input_groups_correctly_but_emits_in_input_order() {
    let input = format!(
        "{HEADER}\n\
         u1\t2026-10-09T12:40:00+09:00\topen\n\
         u1\t2026-10-09T12:00:00+09:00\topen\n\
         u1\t2026-10-09T12:05:00+09:00\tcategory\n"
    );
    let out = sessions(&input, "30m");
    assert!(out.status.success());
    // input-order ids: row0 (12:40) is its own session; row1/row2 (12:00,12:05) share.
    let id = ids(&out);
    assert_ne!(id[0], id[1]);
    assert_eq!(id[1], id[2]);
}

#[test]
fn column_count_mismatch_is_an_error() {
    let input = format!("{HEADER}\nu1\t2026-10-09T12:00:00+09:00\n"); // missing event
    let out = sessions(&input, "30m");
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("column count mismatch")
    );
}

#[test]
fn missing_column_is_an_error() {
    let out = sessions("a\tb\nx\ty\n", "30m");
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("column not found")
    );
}

#[test]
fn unknown_subcommand_is_an_error() {
    let out = run(&["coverage"], "");
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("unknown command")
    );
}

#[test]
fn keys_are_used_verbatim_not_trimmed() {
    // "u1" and "u1 " (trailing space) are different keys — never merged, even 1s apart.
    let input = format!(
        "{HEADER}\n\
         u1\t2026-10-09T12:00:00+09:00\topen\n\
         u1 \t2026-10-09T12:00:01+09:00\topen\n"
    );
    let out = sessions(&input, "30m");
    assert!(out.status.success());
    let id = ids(&out);
    assert_ne!(id[0], id[1]);
}

#[test]
fn multiple_input_files_are_an_error() {
    let out = run(
        &[
            "sessions",
            "a.tsv",
            "b.tsv",
            "--key",
            "user_id",
            "--time",
            "timestamp",
            "--gap",
            "10m",
        ],
        "",
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("multiple input files")
    );
}

#[test]
fn blank_line_is_an_error_not_silently_dropped() {
    let input = format!(
        "{HEADER}\n\
         u1\t2026-10-09T12:00:00+09:00\topen\n\
         \n\
         u1\t2026-10-09T12:00:01+09:00\topen\n"
    );
    let out = sessions(&input, "30m");
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("column count mismatch")
    );
}

#[test]
fn gap_out_of_range_is_an_error_not_panic() {
    let out = sessions(&format!("{HEADER}\n"), "9223372036854775807s");
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("out of range")
    );
}

#[test]
fn bad_duration_is_an_error() {
    let out = sessions(&format!("{HEADER}\n"), "10x");
    assert!(!out.status.success());
    assert!(String::from_utf8(out.stderr).unwrap().contains("duration"));
}
