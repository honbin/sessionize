# sessionize

Assign session IDs to keyed, timestamped events — a small Unix filter in Rust.

A number is only as trustworthy as the definitions under it. One premise hiding under an
aggregate is *how events were grouped into sessions*: change the idle gap and a rate can
move. `sessionize` makes that grouping explicit and reproducible — it assigns a
`session_id` so the metric is computed downstream, and you can re-run with a different
gap and compare.

It reports **facts, not verdicts**. It never says a gap is the right boundary or a
number is wrong — it reproduces a session definition and leaves the judgement to the
downstream analysis.

## Install

```
cargo install --git https://github.com/honbin/sessionize --locked
```

Needs a Rust toolchain; installs the `sessionize` binary to `~/.cargo/bin`.

## Usage

```
sessionize [FILE] --key <col> --time <col> --gap <dur>
```

Reads a header-bearing **TSV** (stdin if `FILE` is omitted), sorts internally, and emits
the input rows unchanged plus a `session_id` column — **in the original row order**.
Events are grouped by key; within each key, a new session starts when the idle gap is
strictly greater than `--gap`.

- `--gap` idle gap: `90s` / `10m` / `2h` (a bare number is seconds).
- Timestamps (`--time`) must be RFC3339 with an explicit offset (`2026-10-10T10:00:00+09:00`
  or `…Z`). Rows are ordered by the parsed instant, not lexicographically, so offsets may
  differ across rows and the same instant in different offsets sorts equal.
- Input contract: the `--key` and `--time` fields contain no tab or newline (true for
  ids and RFC3339 timestamps), and are used **verbatim** — surrounding whitespace is
  significant (` u1` and `u1` are different keys), so normalize upstream if needed.
  Rows whose column count differs from the header are an error, not silently mishandled.

It composes with the usual extraction idiom — reshape the source to TSV upstream, let
the metric live downstream:

```sh
# Normalize events to TSV, assign session IDs, then compute the metric downstream.
# Only --key and --time are read; other columns ride along. Source here is JSON Lines:
{ printf 'user_id\ttimestamp\tevent\n'
  jq -r '[.user_id, .timestamp, .event] | @tsv' events.jsonl
} \
  | sessionize --key user_id --time timestamp --gap 10m \
  | awk -F '\t' '...'   # group by session_id, compute the metric
```

The reshaping step is yours — a different source (a columnar export, a query result
shaped as field/value pairs) needs its own extraction. `sessionize` only asks for the
header-bearing TSV above.

### What it does *not* do

- **It does not prevent reopen-merge.** If a key's events fall within `--gap`, they
  stay in one session, re-opens and all. The point is to make a session definition
  *reproducible*, so you can see how that definition moves a metric — run it twice with
  different `--gap` and compare downstream.
- **It does not judge the definition.** Reproducing what some other code computed, and
  that definition being analytically correct, are different things. `sessionize` only
  gives you the first; the second is yours to argue.

## Build & test

```
cargo test
```

- unit (`src/lib.rs`) — the pure `sessionize` core: grouping, gap split, input-order.
- CLI (`tests/cli.rs`) — the binary end to end: TSV in → session_id out → exit code.

## License

[MIT](LICENSE)
