# pf

A command-line client for the [Passionfroot public API](https://workspace.passionfroot.me),
in the shape of `gh`. Built as a third-party tool: it needs nothing added to
Passionfroot itself, only an API key.

```
pf inbox
pf placement list --status confirmed --start-date 2026-01-01 --paginate
pf creator message creator_abc123 --text "Hi! Would you like to collaborate?"
pf proposal accept prop_123
```

## Install

```sh
cargo install --path .
```

## Authentication

Mint a key at **Settings > API Keys** in your Passionfroot dashboard and export
it:

```sh
export PASSIONFROOT_API_TOKEN=pf_live_...   # or PF_API_TOKEN
```

`pf` reads the token only from the environment. There is no `--token` flag, on
purpose: a secret in `argv` ends up in shell history and in `ps` output.

```
$ pf auth status
✓ https://workspace.passionfroot.me/api/v1
  token pf_live_…8fa2 (env PASSIONFROOT_API_TOKEN)
  32 labels readable
```

## Commands

| | |
|---|---|
| `pf auth status` | check the configured token |
| `pf inbox` | unread conversations, with creator names |
| `pf placement list` | `--start-date --end-date --status --collab --include`, `--metrics-updated-after/-before` |
| `pf creator list` | `--label <id\|name>… --include channels` |
| `pf creator get <id>` | `--include channels` |
| `pf creator message <id>` | send to a creator, whether or not you have talked before |
| `pf label list` | the workspace label catalog |
| `pf collab list` / `get <id>` | `--status --creator --include campaign` |
| `pf conv list` | `--updated-after --creator --search --unread --archived --blocked` |
| `pf conv messages <id>` | `--created-after --type --reverse` |
| `pf conv reply <id>` | send a message |
| `pf conv read <id>` | clear the unread flag |
| `pf inquiry send` | `--creator --text [--campaign]` |
| `pf proposal accept <id>` / `reject <id>` | act on a creator proposal |
| `pf api [METHOD] <path>` | any endpoint directly |

`collaboration`, `conversation`, `placements`, `creators` and `labels` all work
as aliases, as do `ls` for `list` and `view` for `get`.

`pf creator message` exists because there is at most one conversation per
creator. It looks that conversation up and sends into it, or sends an inquiry
when none exists yet — the two cases an agent would otherwise have to branch on
itself. The branch it took is reported on stderr, and as an `action` field
under `--json`.

## Notes for agents

**Output.** Tables on a terminal, JSON when piped, so `pf conv list | jq` needs
no extra flag. JSON is the upstream response unmodified — no field is renamed,
reformatted, or dropped, and cents stay integers.

**Exit codes** are stable and worth branching on:

```
0 ok    2 usage    3 auth (401/403)    4 not found (404)
5 conflict (409/422)    6 rate limited    7 network/timeout    1 other
```

Under `--json`, errors go to stderr in the same shape:

```json
{"error":{"code":"conflict","status":409,"message":"…","hint":"…"}}
```

**Rate limiting.** The API allows 2 requests/second per key behind a
20/second per-IP cap. `pf` throttles itself to that budget, adapts to the
`RateLimit` headers, and honours `Retry-After` on a 429. The throttle is shared
across concurrent `pf` processes through a lock file in your cache directory,
so a shell loop is bounded too — set `PF_NO_SHARED_THROTTLE=1` to opt out.

**Pagination.** `--paginate` walks the cursor to exhaustion and returns one
merged `data` array. `--cursor` resumes from a checkpoint. Cursors are opaque
and single-origin; one the API did not issue is a hard `400`, so `pf` never
constructs, caches, or replays one.

**Idempotency.** Every write carries an `Idempotency-Key`, which makes `pf`'s
own retries safe. It does nothing across separate invocations — re-running
`pf conv reply` sends a second message. Pass `--idempotency-key <key>` to make
a command re-runnable, and reuse that exact key for every retry of the same
logical send.

**`--dry-run`** prints the requests that would be sent, with the token masked,
and sends nothing:

```
$ pf creator message creator_abc123 --text "Hi!" --dry-run
# step 1: decides which of the two branches below runs
GET https://workspace.passionfroot.me/api/v1/conversations?creatorId=creator_abc123&limit=1
  authorization: Bearer pf_live_…8fa2
…
```

**Message text** is sent verbatim. The API accepts an HTML allowlist, so `<br>`
is a line break and a bare newline is not. Pass text with `--text`,
`--text-file <path>`, or on stdin.

## Development

```sh
cargo test
cargo run -- --help
```

`PF_BASE_URL` points the client at another host, which is how the test suite
runs it against a local mock.
