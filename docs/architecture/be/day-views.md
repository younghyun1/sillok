# Day views and trees

Sillok stores no days. A day is a half-open instant window between two local midnights in the zone chosen at query time: `--tz`, then `SILLOK_TZ`, then the system IANA zone (UTC with a warning when the system zone is unknown). Zones that skip midnight for daylight saving start the day at the first valid hour.

## `day`

1. Compute `[start, end)` for the date.
2. Read events with `event_at` in the window (`event_occurred_idx`) and collect an activity list per record: `recorded`, `amended`, `completed` (an amend to `completed`), `moved`, `retracted`, `restored`, each once, in time order.
3. Load those records, drop retracted ones, and add ancestors one level per query (bounded depth). A retracted ancestor stops the walk; its descendants appear as roots.
4. Build the forest: roots are records without a parent in the set; siblings are ordered by `(created_at, id)`.

Records present only as ancestors carry no `activity`, which is how a reader tells context from work done that day. A long-running objective therefore appears on every day something under it happened.

## `tree`

`tree <id>` loads the root and its visible descendants breadth-first, one query per level (`record_parent_idx`), and renders the same node shape without activity.

## `status`

`status` resolves the current repository with two parallel Git queries: its root (else the working directory), the main checkout when the root is a linked worktree, and the sanitized `origin` URL. A context belongs to the repository when it was recorded under either root, in the same directory outside Git, or in any checkout with the same remote, so work from other worktrees, other clones, and other machines is included. `status` lists the most recently touched records with events in those contexts, and the open objectives that were created there or are parents of that recent work. `--all` drops the filter.
