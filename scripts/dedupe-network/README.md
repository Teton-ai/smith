# Network Table Deduplication

One-off tooling to collapse duplicate `network` rows. Read this before running
anything: phases 3 and 4 write to a database Smith does not own.

## Background

The `network` table accumulated duplicates because `POST /networks` used to
insert unconditionally, with no uniqueness constraint on the content.

The App API `departmentNetworkSmithSync` job calls `POST /networks` to register
a WiFi credential, then writes the returned `id` to its `network_smith` bridge
table. If the bridge write failed after Smith had created the row, the wifi
entry stayed in the retry set (`network_smith_id IS NULL`) and the job inserted
another row on the next pass, every 15 minutes. The table reached 725 rows for
roughly 130 distinct networks.

`POST /networks` and `ReportNMProfiles` are now content-addressed and converge on
one row, so the table no longer grows this way. This tooling cleans up what is
already there.

## The content key

Phases 2, 3 and 4 all group on the same tuple, under **strict** equality (NULL
equal to NULL). It is defined once as `NETWORK_CONTENT_KEY` in `_common.sh`:

```
(ssid, is_network_hidden, credentials->>'psk', network_type, security_type, identity)
```

This is deliberately stricter than the API's own match. `network_find_by_content`
relaxes `security_type` and `identity` bidirectionally, so a NULL matches
anything. That relation is **not transitive**: `open ~ NULL ~ wpa-eap`, but
`open !~ wpa-eap`. It therefore cannot be expressed as a `GROUP BY` at all.

Grouping strictly means rows that differ only by a relaxable NULL are left alone
rather than merged on a rule the scripts cannot express. That residue is small
and is meant to be handled by hand.

Two consequences worth knowing:

- `name` and `description` are **not** in the key. The survivor keeps its own and
  the other is dropped. App-API-created rows always carry `name = ssid` and
  `description = 'Network for <ssid>'`, so merging tends to replace hand-written
  Smith labels with that boilerplate.
- `credentials` is keyed on `->>'psk'` only. Its other keys (`pmf`, `eap`,
  `phase2_auth`, `anonymous_identity`) are unprotected and would be dropped with
  the row, so phases 2 and 3 fold them into the survivor before deleting. None of
  them affect matching, so the next device report would restore them anyway; the
  fold matters when a merged network's devices have gone quiet.

  Only `psk` is guaranteed equal across a group, so the other keys can conflict.
  Two rules resolve it: `merged || k.credentials` puts the survivor last, so its
  own values always win, and `jsonb_object_agg(... ORDER BY o.id)` lets later
  keys overwrite earlier ones, so among the dropped rows the newest wins
  deterministically.

## Repointing foreign keys

Phases 2 and 3 both remap the four device-side FKs (`device.network_id`,
`device.current_network_id`, `device_configured_network.network_id`,
`device_network_intent.network_id`) with a plain `UPDATE` before deleting.

`network_reference` needs different handling. Its FK is `ON DELETE RESTRICT`, so
it must be repointed before the delete or the whole transaction aborts, and its
primary key is `(holder, external_key, network_id)`, so a reference that already
exists for the canonical id would collide on the way over. Both scripts therefore
insert what is missing with `ON CONFLICT DO NOTHING` and then drop the old rows
outright, rather than updating in place.

## Connecting

Every script takes an optional Smith URL as its last positional argument and
accepts two forms:

- `postgres://...` an ordinary TCP connection
- `docker://<container>[/<database>]` runs psql inside the container

The `docker://` form exists because a full-tunnel VPN (Cisco Secure Client)
installs routes for the Docker bridge subnets pointing at the tunnel, so host
connections to a published port hang with no RST. Reaching the prod App API and
the local Smith DB at the same time requires it. Default is
`docker://smith-postgres`.

Every phase script fetches the App API refs live. Their delete guard is "the App
API references this row", and the sync job bridges new rows continuously, so a
stale copy of the refs would put real rows at risk.

## Phases

Run them in order. Every script dry-runs by default and needs `--commit` to
apply. Always read the plan first.

### Phase 1: delete unreferenced rows

`phase1-delete-unreferenced.sh`

Deletes every `network` row with no reference in any of `device.network_id`,
`device.current_network_id`, `device_configured_network.network_id`,
`device_network_intent.network_id`, `network_reference.network_id`, or the App
API `network_smith` bridge.

`network_reference` matters out of proportion to its size: its FK is
`ON DELETE RESTRICT`, so a row holding one cannot be deleted even when every
other reference count is zero. It is empty in prod today.

No grouping and no canonical rule are involved, so it is safe to run before any
of the key questions are settled.

Two details in the delete condition:

- The id watermark is taken **before** the ref fetch, not after, so any row
  created while the run is in progress is out of scope for the delete no matter
  how long the run takes.
- Empty-string passwords are normalized to NULL first, so open networks compare
  consistently across every phase. `network.password` is still written by the API
  alongside `credentials`, so this is live, not legacy cleanup.
- The `dup_group` column in the report uses the same content key as phases 2 to
  4, so a group shown here is the group those phases will act on.

```bash
./scripts/dedupe-network/phase1-delete-unreferenced.sh <app-api-db-url> [<smith-db-url>] [--commit]
```

### Phase 2: deduplicate referenced rows

`phase2-dedupe-referenced.sh`

Groups the survivors on the content key above and merges each group onto one
canonical row, remapping FKs and folding credentials before deleting the rest.

Canonical selection, for groups the phase can fully merge:

1. The row referenced by App API `network_smith.network_smith_id`.
2. Then the row with the most Smith-side FK references.
3. Then `min(id)`.

Groups where **more than one** row holds an App API ref cannot be fully merged,
because collapsing them needs App API writes. Those are skipped and left to
phase 3, except for the safe part: any row in them with no App API ref is folded
into a bridged row of the same group and deleted.

That fold is not the canonical rule above. Phase 2 is not choosing a survivor
there, since every bridged row in a skipped group is staying put regardless; it
is parking device-only rows somewhere valid until phase 3 does the real merge,
and any bridged row in the group is equally correct. It picks by
`smith_refs DESC, id ASC`, the same order phase 3 uses for its canonical, for two
reasons: the parked references land on the row phase 3 will keep, so they are not
rewritten twice, and parking cannot inflate an arbitrary row past the group's
real leader and steer phase 3 onto it.

```bash
./scripts/dedupe-network/phase2-dedupe-referenced.sh <app-api-db-url> [<smith-db-url>] [--commit]
```

### Phase 3: deduplicate skipped groups (writes to the App API)

`phase3-dedupe-skipped-groups.sh`

Handles the groups phase 2 skipped, by repointing the App API bridge rows at one
canonical Smith id.

#### Bridge classification

Phase 3 only ever repoints a bridge **within one content group**, so the plan is
only correct if the bridge belongs to that group at all. The App API record's own
fields cannot answer that. The sync job is create-only (`getUnsyncedNetworkWifis`
selects `WHERE ns.network_smith_id IS NULL`), so a record edited after it was
first synced keeps its old bridge, and its ssid, password and security drift away
from the Smith row it points at. Comparing those fields tells you the record
changed, not where its devices are.

Device evidence does answer it. Each bridge is tested against the devices its
owning department actually has, joining App API `device.serial_number` to Smith
`device.serial_number`. A device counts as being on a content group through
`device.network_id`, `device.current_network_id`, or
`device_configured_network.network_id`. `device_network_intent` is deliberately
excluded: it is operator intent, not evidence of where the device is.

| class | meaning | action |
|---|---|---|
| `CONFIRMED` | the department has devices on this content group | repoint to canonical, delete the emptied row |
| `UNKNOWN` | the department yields no evidence at all: no devices in Smith, or none of its devices reference any network row | repoint, and report. Nothing contradicts the bridge, and refusing to act on absent evidence would strand the group forever |
| `UNSUPPORTED` | the department has devices on some content group, and it is not this one | **held back.** The row keeps its bridge and is not deleted |

An `UNSUPPORTED` bridge points at a group its department is not on, so every
destination inside that group is equally wrong and repointing would relocate a
bad link rather than fix it. It cannot be repaired here at all: the repair has to
leave the content group, and no Smith-side write can express that. Phase 3
reports these and moves on.

A row is held back if **any** of its bridges is `UNSUPPORTED`, because the remap
deletes the row and would carry the bad bridge to the canonical with it. A
held-back row may still be chosen as canonical; that only means it survives,
which it does either way.

The report prints the classification per row, plus an `UNSUPPORTED BRIDGES`
section covering the whole bridge set. Its `blocks_merge` column marks the ones
that also stop a phase 3 group from collapsing; the rest are bridges in
single-row groups, detected but not in phase 3's way.

#### Canonical selection

Most Smith-side references, then `min(id)`. Every candidate holds an App API ref
here, so phase 2's first tiebreak is dead weight. Choosing `min(id)` alone is
valid but repoints far more FK rows for no benefit, since the rows are
content-identical by construction. The `min(id)` tiebreak only breaks ties in the
reference count; in the current data the count is decisive in every group and it
never fires.

#### Order of operations

This matters because the two commits cannot be atomic:

1. Classify the bridges (read-only).
2. Print the plan (read-only, rolls back).
3. Update App API `network_smith.network_smith_id`, then assert no bridge still
   points at a row the next step deletes. The assertion `RAISE`s rather than
   printing a count, so a partial update aborts the run instead of letting the
   Smith step strand the bridges it missed.
4. Remap Smith FKs, fold credentials, delete non-canonical Smith rows.

App API goes first so a failure fails recoverably: the bridge points at the
canonical row, which still exists, and re-running converges. The reverse order
fails toward dangling bridges, App API rows pointing at deleted Smith ids, which
nothing here can detect afterwards.

#### Payload handling

Every payload is streamed into psql from a **quoted** heredoc. Device serial
numbers are free text and real values contain spaces, commas, `+` and `/`, so an
unquoted heredoc would let bash expand a serial containing `$` or a backtick.
Values reach the server as `COPY` data and never become SQL program text.

```bash
./scripts/dedupe-network/phase3-dedupe-skipped-groups.sh <app-api-db-url> [<smith-db-url>] [--commit]
```

Phase 3 reads four App API tables: `network_smith` (the bridge it rewrites),
`network_wifis` and `network_surveys` (which department owns each bridge), and
`device` (the evidence). It writes only `network_smith.network_smith_id`.

Pause `departmentNetworkSmithSync` for this run if you can.

### Phase 4: repair stale bridges (writes to the App API)

`phase4-repair-stale-bridges.sh`

Picks up what phases 1 to 3 could not deal with: the bridges phase 3 classified
`UNSUPPORTED` and held back. Repoints the ones with a computable destination,
then deletes the Smith rows that repair leaves unreferenced.

#### The resolution test

A destination is only defensible when two independent things agree:

1. The App API record's **own** content (`ssid`, `hidden`, `password`) matches a
   content group, and
2. that group is one the owning department's devices are actually on.

Content alone is what went stale in the first place. Device evidence alone
cannot say which of several networks a record meant. Requiring both, and
requiring **exactly one** candidate group, is what makes this a repair rather
than a guess. Open networks store `password = ''` on the App API side and a NULL
psk in Smith, so the fetch normalizes with `NULLIF`.

Bridges with zero candidate groups are contradictory, not ambiguous: the record
describes a network that exists nowhere its devices are, sometimes nowhere in
Smith at all. Those are printed with the evidence and left untouched.

`security` is deliberately not part of the match. It adds nothing here, since
ssid plus password plus the device constraint already pin the group, and Smith
rows may carry a NULL `security_type` that no App API value equals.

#### The delete

Scoped to the rows this run freed. A general sweep is phase 1's job. A row is
deleted only when it falls to zero references across all six sources and sits
below the id watermark, which is taken before the first fetch.

The guard excludes the bridges being repaired rather than reading them back from
the App API, so a dry run reports exactly what a commit would do. Refs are
re-fetched immediately before the delete to narrow the sync-job race.

#### Order of operations

Same reasoning as phase 3: App API first, so a later failure leaves a bridge
pointing at a row that still exists, and the same post-update assertion aborts
the run rather than deleting rows whose bridges did not move.

The canonical rule here is deliberately **not** phase 3's. Phase 3 picks only
among bridged rows, because in a skipped group every candidate holds a bridge. A
phase 4 destination group may hold no bridge at all, so the whole group is in
play and the choice falls to Smith references, then `min(id)`.

```bash
./scripts/dedupe-network/phase4-repair-stale-bridges.sh <app-api-db-url> [<smith-db-url>] [--commit]
```

Reads the same four App API tables as phase 3, plus `network_wifis.ssid`,
`hidden` and `password`. **That payload carries live WiFi passwords**: it is
streamed as `COPY` data, never echoed, and never interpolated into SQL text.

## Shared helpers

`_common.sh` is sourced by all four phase scripts, never executed. It provides:

- `db_psql`, the `docker://` handling described above.
- The App API fetchers. `network_smith_id` is a TEXT column, so integer payloads
  are validated as such; the device and wifi-content payloads are free text and
  are streamed instead.
- `copy_block`, which emits a `\copy` command, its payload and its terminator
  between quoted heredocs.
- `NETWORK_CONTENT_KEY` and `emit_referenced`. The content key is written **once**
  here and used by phases 2, 3 and 4, so the invariant the whole toolchain rests
  on cannot drift between scripts. `emit_referenced` builds `_referenced`, every
  row anything points at with its group and reference counts. It is a temp table
  rather than a view because callers scan it repeatedly and every count is a
  correlated subquery.
- The race mitigations below.

## Races against the sync job

`departmentNetworkSmithSync` calls `POST /networks` and then writes the bridge
row. Between those two steps the Smith row exists and is unreferenced, so a
dedupe run in that window can delete a row the App API is about to point at.

Two windows:

- A row created **after** the run started. Closed deterministically by an id
  watermark (`network.id` is `generated always as identity`, so it is monotonic;
  the table has no `created_at`).
- A row that already existed and is bridged **after** the ref fetch. Narrowed by
  fetching refs twice and taking the union.

Neither substitutes for pausing the job. Window B stays open for the duration of
the transaction itself, which no client-side scheme can close.
