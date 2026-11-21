# Sillok

Sillok is the user's chronicle of agent work: objectives, the tasks done under them, and what happened each day. Record as you go; the chronicle, not chat history, survives sessions, context compaction, and machines.

## Session start

```bash
sillok status    # open objectives and recent work for this repository, across its worktrees and clones
```

Resume the open objective that fits the task (`sillok show <id>` gives its history), or start one. Writes print only the affected id:

```bash
obj=$(sillok objective add "Ship the storage refactor" --tags rust)
```

## While working

```bash
sillok note "Split reducer from indexing" --parent "$obj" --tags rust   # finished work
sillok note "Tracing the flaky sync test" --parent "$obj" --status active   # in flight: open, active, or blocked
sillok amend <id> --status completed --note "Root cause: clock skew"   # also --text, --tags, --purpose
sillok move <id> --parent <other>   # or --top
sillok retract <id> --reason "Recorded under the wrong objective"   # hides it; `restore <id>` undoes
sillok objective complete "$obj" --note "What shipped and what remains"
```

- Record each unit of work when it finishes, and before ending any turn that changed files or systems; do not batch across turns. If a hook sends the turn back asking for a record, write it, or finish when nothing substantive happened.
- Record corrections: when you revise an earlier claim or the user says you were wrong, add a note tagged `correction` under the affected objective, and fix the wrong record with `amend` or `retract`.
- Close what you open: complete an objective when its scope is done, and move `active` tasks to `completed` or `blocked`. Objectives span days; never re-create one per day.
- Keep text factual and self-contained: what changed and why, with identifiers glossed. Never record secrets, tokens, or credentials.

## Reading

```bash
sillok day [--date YYYY-MM-DD]   # every record touched that day, as a tree with per-record activity
sillok show <id>                 # record, children, full event history
sillok tree <id>                 # record and its descendants
sillok objective list            # open objectives everywhere; --all adds completed ones
sillok query --text sync --open --limit 20   # also --since, --from/--to, --date, --tag, --kind, --status, --context
```

Reads print one line of compact JSON without empty fields. `--full` adds the work context (directory, Git state, session), `--json` the `{ok, command, data, warnings}` envelope, and `--human` text for people.

## Rules

- Errors are JSON on stderr, `{"error":"code","message":"..."}`, with exit code 1, or 2 for usage errors. Branch on `error`, not the message.
- `retracted` is set only by `retract`; `note` and `amend` reject it.
- Backfill with `--at 2026-09-29T16:45:00`; naive times use `--tz` or `SILLOK_TZ`, else the system zone.
- `SILLOK_SESSION` labels events with the agent session and `SILLOK_ACTOR` names the writer; set them when the harness exposes them.
- Parallel agents are safe: commands wait for each other instead of failing.
- `sillok sync` exchanges events through the configured Git remote and never prompts; run it only when the user's workflow calls for it.
- Never run `sillok reset --yes` unless the user explicitly asks to erase the chronicle.
- A project instruction that opts out of Sillok overrides this guide.
- A rejected command means the binary on `PATH` is older than this guide; check `sillok --version`.

`sillok guide` prints this text from the installed binary, and `sillok skill install` installs it as an agent skill.
