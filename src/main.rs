//! CLI for `sessionize`: TSV in, the same rows plus `session_id` out. The core is in
//! `src/lib.rs`; usage and the input contract are in the README.

use chrono::{DateTime, Duration, FixedOffset};
use sessionize::{session_count, sessionize};
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::process::ExitCode;

const HELP: &str = "\
sessionize — assign a session_id to keyed, timestamped events

usage:
    sessionize [FILE] --key <col> --time <col> --gap <dur>

Reads a header-bearing TSV (stdin if FILE is omitted), sorts internally, and emits the
input rows unchanged plus a session_id column, in the original row order. A new key or
an idle gap strictly greater than --gap starts a new session.

    --gap   idle gap: 90s / 10m / 2h (bare number = seconds)

Facts only: it does not judge whether the gap is the right boundary, and does not
prevent reopen-merge — it reproduces a definition so its effect on a metric can be
checked downstream. Run it with different --gap and compare.

    cat events.tsv | sessionize --key user_id --time ts --gap 10m";

struct Args {
    file: Option<String>,
    key: String,
    time: String,
    gap: Duration,
}

fn parse_dur(s: &str) -> Result<Duration, String> {
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = num.parse().map_err(|_| format!("bad duration: {s}"))?;
    // try_* so an out-of-range value is a normal arg error, not a panic.
    let d = match unit {
        "h" => Duration::try_hours(n),
        "m" => Duration::try_minutes(n),
        "s" | "" => Duration::try_seconds(n),
        _ => return Err(format!("bad duration unit in {s} (use h/m/s)")),
    };
    d.ok_or_else(|| format!("duration out of range: {s}"))
}

fn set_once<T>(slot: &mut Option<T>, val: T, flag: &str) -> Result<(), String> {
    if slot.replace(val).is_some() {
        return Err(format!("{flag} specified more than once"));
    }
    Ok(())
}

/// `Ok(None)` means help was requested.
fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
    let (mut file, mut key, mut time, mut gap) = (None, None, None, None);
    while let Some(a) = it.next() {
        let mut next = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" => return Ok(None),
            "--key" => set_once(&mut key, next("--key")?, "--key")?,
            "--time" => set_once(&mut time, next("--time")?, "--time")?,
            "--gap" => set_once(&mut gap, parse_dur(&next("--gap")?)?, "--gap")?,
            other if other.starts_with('-') => return Err(format!("unknown flag: {other}")),
            other => {
                if file.replace(other.to_string()).is_some() {
                    return Err("multiple input files are not supported".into());
                }
            }
        }
    }
    Ok(Some(Args {
        file,
        key: key.ok_or("--key is required")?,
        time: time.ok_or("--time is required")?,
        gap: gap.ok_or("--gap is required")?,
    }))
}

fn col(header: &[&str], name: &str) -> Result<usize, String> {
    let mut found = header.iter().enumerate().filter(|(_, h)| **h == name);
    match (found.next(), found.next()) {
        (Some((i, _)), None) => Ok(i),
        (None, _) => Err(format!("column not found: {name}")),
        (Some(_), Some(_)) => Err(format!("duplicate column: {name}")),
    }
}

fn read_input(file: &Option<String>) -> Result<String, String> {
    let mut s = String::new();
    match file {
        Some(p) => std::fs::File::open(p)
            .and_then(|mut f| f.read_to_string(&mut s))
            .map(|_| ())
            .map_err(|e| format!("{p}: {e}"))?,
        None => io::stdin()
            .lock()
            .read_to_string(&mut s)
            .map(|_| ())
            .map_err(|e| format!("stdin: {e}"))?,
    }
    Ok(s)
}

fn write_rows(header_line: &str, raws: &[&str], ids: &[u64]) -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "{header_line}\tsession_id")?;
    for (raw, id) in raws.iter().zip(ids) {
        writeln!(out, "{raw}\t{id}")?;
    }
    out.flush()
}

fn run_sessionize(args: Args) -> Result<(), String> {
    let input = read_input(&args.file)?;
    // Don't skip blank lines — a blank row is a column-count mismatch (caught below),
    // not something to silently drop.
    let mut lines = input.lines();
    let header_line = lines.next().ok_or("empty input")?;
    let header: Vec<&str> = header_line.split('\t').collect();
    let ki = col(&header, &args.key)?;
    let ti = col(&header, &args.time)?;
    // Appending a second session_id would leave downstream lookups by name ambiguous,
    // which is exactly what chaining runs with different --gap produces.
    if header.contains(&"session_id") {
        return Err(
            "input already has a session_id column; drop it upstream before re-running".into(),
        );
    }

    // Keep rows in INPUT order; session ids are mapped back onto this order.
    let mut raws: Vec<&str> = Vec::new();
    let mut events: Vec<(&str, DateTime<FixedOffset>)> = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != header.len() {
            return Err(format!(
                "column count mismatch: header has {}, row has {}: {:?}",
                header.len(),
                f.len(),
                line
            ));
        }
        // Values are used verbatim (no trim): the key is an identity, so trimming
        // would silently merge " u1" and "u1". Normalize upstream if needed.
        let time = DateTime::parse_from_rfc3339(f[ti])
            .map_err(|e| format!("bad time {:?}: {e}", f[ti]))?;
        events.push((f[ki], time));
        raws.push(line);
    }

    let ids = sessionize(&events, args.gap);
    debug_assert_eq!(ids.len(), raws.len());

    match write_rows(header_line, &raws, &ids) {
        Ok(()) => {}
        // The reader closed early (`| head`): not a failure of this filter.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
        Err(e) => return Err(e.to_string()),
    }

    let keys: HashSet<&str> = events.iter().map(|(k, _)| *k).collect();
    eprintln!(
        "events {} / keys {} / sessions {}",
        events.len(),
        keys.len(),
        session_count(&ids)
    );
    Ok(())
}

fn run() -> Result<(), String> {
    match parse_args(std::env::args().skip(1))? {
        Some(args) => run_sessionize(args),
        None => {
            println!("{HELP}");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sessionize: {e}");
            ExitCode::FAILURE
        }
    }
}
