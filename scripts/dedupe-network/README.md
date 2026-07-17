# Network Table Deduplication

## Background

The `network` table accumulated duplicate rows because `POST /networks` has no
uniqueness constraint on `(ssid, password, is_network_hidden)`. Every call
unconditionally inserts a new row.

The App API `departmentNetworkSmithSync` job calls `POST /networks` to register
a WiFi credential, then writes the returned `id` to the `network_smith` bridge
table. If the bridge write fails after Smith has already created the row, the
wifi entry stays in the retry set (`network_smith_id IS NULL`) and the job
inserts another row on the next run (every 15 minutes).

As a result, the table grew to 725 rows despite only ~130 genuinely distinct
networks.

## Phases

### Phase 1: Delete unreferenced rows

The vast majority of duplicates were never assigned to any device and never
recorded in the App API bridge table. They have zero references anywhere and
can be deleted safely with no FK updates.

Script: `phase1-delete-unreferenced.sh`

#### Usage

```bash
# Dry run against local dev Smith DB (default)
./scripts/dedupe-network/phase1-delete-unreferenced.sh <app-api-db-url>

# Dry run against prod Smith DB
./scripts/dedupe-network/phase1-delete-unreferenced.sh <app-api-db-url> <smith-db-url>

# Commit against prod Smith DB
./scripts/dedupe-network/phase1-delete-unreferenced.sh <app-api-db-url> <smith-db-url> --commit
```

The script fetches fresh App API refs at runtime so it is safe to re-run at
any time. Always do a dry run first.

#### What it does

1. Pulls the current list of `network_smith_id` values from the App API
   `network_smith` table.
2. Deletes every `network` row that has no reference in any of:
   - `device.network_id`
   - `device.current_network_id`
   - `device_configured_network.network_id`
   - `device_network_intent.network_id`
   - App API `network_smith.network_smith_id`

#### What it does NOT do

It does not touch any referenced rows. After phase 1, the remaining rows are
exactly those visible in the cross-analysis query (`network_cross_analysis.sql`):
networks that are either assigned to a device or known to the App API.

Expected result: ~130 rows remaining.

### Phase 2: Deduplicate referenced rows

Among the ~130 rows that remain after phase 1, some are still duplicates of each
other (same ssid/password/hidden, different IDs). These have FK references and
need careful remapping before the non-canonical rows can be deleted.

Script: `phase2-dedupe-referenced.sh`

#### Canonical ID selection

Within each duplicate group (same ssid/hidden/password):

1. The ID currently referenced by `network_smith.network_smith_id` in the App API wins.
2. Fallback: the ID with the most Smith-side FK references across `device.network_id`,
   `device.current_network_id`, `device_configured_network.network_id`, and
   `device_network_intent.network_id`.
3. Tie-break: min(id).

Groups where more than one row is each referenced by a different App API entry
cannot be fully merged: the App API-referenced rows are left as-is (merging them
would require updating the App API bridge table). However, any device-only row
(no App API ref) within such a group is remapped to the lowest App API-referenced
ID in the group and deleted. No App API writes are required for this.

#### Usage

```bash
# Dry run against local dev Smith DB (default)
./scripts/dedupe-network/phase2-dedupe-referenced.sh <app-api-db-url>

# Dry run against prod Smith DB
./scripts/dedupe-network/phase2-dedupe-referenced.sh <app-api-db-url> <smith-db-url>

# Commit against prod Smith DB
./scripts/dedupe-network/phase2-dedupe-referenced.sh <app-api-db-url> <smith-db-url> --commit
```

Always run phase 1 before phase 2. Always do a dry run first and inspect the
remapping plan before committing.

### Phase 3: Deduplicate skipped groups (requires App API writes)

Some duplicate groups cannot be resolved by phase 2 alone: every row in the
group is referenced by a different App API `network_smith` entry. Merging them
requires updating the App API bridge table so all those entries point to one
canonical Smith ID.

Script: `phase3-dedupe-skipped-groups.sh`

#### Canonical ID selection

Within each skipped group: min(id) among App API-referenced rows.

#### What it does

1. Queries Smith (read-only) to compute the old→canonical remapping for all
   skipped groups.
2. Updates `network_smith.network_smith_id` in App API for each non-canonical
   row to point to the canonical Smith ID.
3. Remaps Smith FKs (`device`, `device_configured_network`,
   `device_network_intent`) and deletes non-canonical Smith rows.

App API is updated first. If the Smith step fails after the App API commits,
the canonical Smith row still exists so the state remains consistent.

#### Usage

```bash
# Dry run against local dev Smith DB (default)
./scripts/dedupe-network/phase3-dedupe-skipped-groups.sh <app-api-db-url>

# Dry run against prod Smith DB
./scripts/dedupe-network/phase3-dedupe-skipped-groups.sh <app-api-db-url> <smith-db-url>

# Commit against prod Smith DB
./scripts/dedupe-network/phase3-dedupe-skipped-groups.sh <app-api-db-url> <smith-db-url> --commit
```

Run phases 1 and 2 before this script. Always dry-run first.
