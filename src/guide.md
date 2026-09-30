# Sillok agent guide

Sillok is a chronicle of your work: objectives, the tasks done under them, and what happened each day. Record as you go; do not rely on chat history.

## Session start

```bash
sillok status                 # open objectives and recent work in this repository
sillok objective list         # every open objective, with ids
```

Resume an open objective by id, or start one. Writes print only the new id:

```bash
obj=$(sillok objective add "Ship the storage refactor" --tags rust)
```

## While working

```bash
sillok note "Split reducer from indexing" --parent "$obj" --tags rust
sillok note "Investigating flaky sync test" --parent "$obj" --status active
sillok amend <id> --status completed --note "Root cause: clock skew"
sillok move <id> --parent <other_id>        # or --top
sillok retract <id> --reason "Recorded under the wrong objective"
sillok restore <id>
sillok objective complete "$obj" --note "All scoped work is done"
```

Objectives and tasks span days. A record shows up in `sillok day` for every day it had an event.

## Reading

```bash
sillok day                          # today, as a tree with per-record activity
sillok day --date 2026-09-29
sillok show <id>                    # record, children, full event history
sillok tree <id>
sillok query --text "sync" --open   # also --since, --date, --tag, --kind, --status, --limit
```

Reads print compact JSON; add `--full` for work context, `--json` for the envelope, `--human` for text.

## Rules

- Errors are JSON on stderr: `{"error":"code","message":"..."}`. The exit code is 1, or 2 for usage errors.
- Status `retracted` is only set by `retract`, which keeps a reason.
- Backfill with `--at 2026-09-29T16:45:00` and set the day zone with `--tz` or `SILLOK_TZ`.
- Set `SILLOK_SESSION` to group events by agent session and `SILLOK_ACTOR` to name the writer.
- Never run `sillok reset --yes` unless the user explicitly asks to erase the chronicle.
- Many agents may write at once; commands wait for each other instead of failing.
