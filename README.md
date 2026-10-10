# data-probe

Small tools to probe the premises an aggregate rests on — written in Rust.

A number is only as trustworthy as the data and the definitions under it. Before you
read a rate, a median, or a funnel, it helps to ask: who is in the denominator, what
was censored, and does the number move if you count a little differently? `data-probe`
turns some of those questions into small, composable commands.

It reports **facts, not verdicts**. It never says a number is wrong or a definition is
right — it surfaces what the number depends on and leaves the judgement to you and the
downstream analysis.

## A box, not a framework

There is one subcommand today:

| subcommand | what it does |
|------------|--------------|
| `sessions` | assign a `session_id` to keyed, timestamped events under one idle-gap definition |

More probes get added only when a real analysis needs one — the name leaves room for a
future, the implementation is decided by present need. An empty-ish box is fine; this
is a place to put tools that became necessary, not a reason to grow them.

## Install

```
cargo install --git https://github.com/honbin/data-probe --locked
```

Needs a Rust toolchain; installs the `data-probe` binary to `~/.cargo/bin`.

## sessions

```
data-probe sessions [FILE] --key <col> --time <col> --gap <dur>
```

Reads a header-bearing **TSV** (stdin if `FILE` is omitted), sorts internally, and
emits the input rows unchanged plus a `session_id` column — **in the original row
order**. Events are grouped by key; within each key, a new session starts when the
idle gap is strictly greater than `--gap`.

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
  | data-probe sessions --key user_id --time timestamp --gap 10m \
  | awk -F '\t' '...'   # group by session_id, compute the metric
```

The reshaping step is yours — a different source (a columnar export, a query result
shaped as field/value pairs) needs its own extraction. `data-probe` only asks for the
header-bearing TSV above.

(`awk`'s default field split is whitespace, not TSV — use `-F '\t'` so empty fields
don't shift columns.)

### What it does *not* do

- **It does not prevent reopen-merge.** If a key's events fall within `--gap`, they
  stay in one session, re-opens and all. The point is to make a session definition
  *reproducible*, so you can see how that definition moves a metric — run it twice with
  different `--gap` and compare downstream.
- **It does not judge the definition.** Reproducing what some other code computed, and
  that definition being analytically correct, are different things. `data-probe` only
  gives you the first; the second is yours to argue.

## Build & test

```
cargo test
```

- unit (`src/lib.rs`) — the pure `sessionize` core: grouping, gap split, input-order.
- CLI (`tests/cli.rs`) — the binary end to end: TSV in → session_id out → exit code.

## License

[MIT](LICENSE)
