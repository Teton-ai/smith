# RFC-003: WiFi network management (intent, apply, and the catalog)

**Date:** 2026-07-03
**Status:** Request for comments (comment deadline: 2026-07-10)
**Branch:** feat/apply-new-wifi
**Author:** Antoine Wajntraub

## TL;DR

We can now *see* what WiFi state a device has (#477, #480). This RFC proposes how we *change* it: a declarative, versioned "apply" mechanism modeled on Kubernetes' spec/status pattern, rolled out in three phases with no flag-day deploy. Phase 0 is manual per-device apply (fixes stranded devices, zero mass-reconnect risk); phase 1 adds trust signals for our (currently unreliable) network catalog; phase 2 lets the App API assign department default networks automatically.

**What I need from you:** read the Decisions section; anything tagged `[input wanted]` is genuinely open. Everything tagged `[settled unless objected]` ships as described if nobody pushes back by the deadline. The four Open Questions at the end need actual answers, not just absence of objection.

## The problem, as two stories

**The stranded device.** A device is BLE-provisioned at install with a typo in the WiFi password. It never comes online. BLE provisioning is effectively one-shot, so the only fix is a tech physically plugging in a 4G dongle. Once the device is reachable, Smith has no practical way to fix the broken NM profile: the existing `UpdateNetwork` flow works, but only through the raw API (no dashboard surface), it cannot set up a new nmcli configuration, and it deletes the existing profile before creating the new one, which is exactly the wrong order for a device with one shot at connectivity.

**The rotten password.** A customer's IT rotates the WiFi password and staff update it in the app. The App API's sync to Smith is create-only, so Smith's `network` row keeps the old password forever. An operator assigning that network from the dashboard pushes a dead credential with full confidence. Newly provisioned devices at the same site get the new password via BLE, so the fleet forks into two realities depending on install date, and nothing in Smith can tell the difference.

## Facts (from code and prod, 2026-07-03)

- **Reality reporting is solved in code, not yet in prod.** #477 (merged) and #480 (in review) add `device_configured_network` (NM profiles incl. PSKs, keyed by profile name), `device.current_network_id`, event-driven reports via the nm_watcher D-Bus actor (debounced, fires on NM state/profile changes), and on-demand WiFi scans. Reports are event-driven: a stable device generates none.
- **Intent is essentially unused.** Of 2995 live devices (approved, not archived, seen in 24h), 139 have a `network_id`, pointing at 7 distinct networks. `UpdateNetwork` (raw API only, see the first story) supports no operator workflow.
- **The catalog is polluted and stale.** `network` has 663 rows for 72 real (ssid, password) pairs; ~590 are duplicates from a 15-hour sync incident (see #481). The App API sync (`api` repo, `src/features/networks/job.ts`) only pushes rows with no bridge record (`WHERE network_smith_id IS NULL`), so edits and password rotations never propagate. The table has no timestamps, so none of this can even be dated from the data itself.
- **BLE bypasses Smith entirely.** The provisioning payload comes from App API `network_wifis` via `GET /departments/:id/network/broadcast` (first `available = true` wifi of the department). Smith can only learn about the resulting profile from device reports (#477, not yet in prod), so today prod Smith has no visibility into BLE-provisioned profiles.
- **The App API does not set device intent today.** The internal docs claim it calls `PUT /devices/{serial}/network`; no such call exists in the codebase. Its only Smith writes are network creation (the sync job) and device labels. Operators are the only intent writers.

## Prior art (why this shape)

This is the desired-state vs observed-state problem that Kubernetes (spec/status, `generation`/`observedGeneration`, server-side apply field ownership), AWS IoT Device Shadow (desired/reported with document versions), and Red Hat Flight Control (the same model on Postgres, for edge devices) all solve the same way: separate storage for intent and reality with disjoint writers, a version counter echoed by the device instead of value diffing, and reconciliation that owns only what it created. Argo CD's UI (one derived sync status, diff on demand) is the reference for presenting it. Links in the appendix. Given our ambition to be "Kubernetes for IoT", staying close to their behavior is a feature in itself.

## Design overview

Three layers, each with its own tables and its own writers:

```text
┌─ CATALOG ──────────────────┐  ┌─ INTENT (per device) ──────┐  ┌─ REALITY (per device) ─────┐
│ which credentials exist    │  │ what the device SHOULD have│  │ what the device HAS        │
│                            │  │                            │  │                            │
│ table:                     │  │ table:                     │  │ tables:                    │
│   network                  │  │   device_network_intent    │  │   device_configured_network│
│   + source                 │  │   + device.intent_version  │  │   device.current_network_id│
│   + source_updated_at      │  │                            │  │   + device.observed_       │
│   + created_at             │  │                            │  │       intent_version       │
│                            │  │                            │  │   + device.network_        │
│                            │  │                            │  │       conditions           │
│ written by:                │  │ written by:                │  │ written by:                │
│   App API sync,            │  │   operators (now),         │  │   device response path,    │
│   operators,               │  │   App API defaults         │  │   NOTHING else             │
│   device reports           │  │   (phase 2)                │  │                            │
└────────────────────────────┘  └────────────────────────────┘  └────────────────────────────┘
```

The apply loop that connects intent and reality, step by step:

1. An operator edits the device's intent list; the API bumps `device.intent_version` (say 6 → 7).
2. The operator presses Apply; the API resolves credentials from the catalog and queues `ApplyNetworks { version: 7, networks: [...] }` to the device.
3. smithd reconciles its NM profiles against the list (by SSID) and persists "I applied version 7" to disk.
4. From then on, every profile report the device sends (startup, NM change, on demand) carries `applied_version: 7` plus one condition per SSID (`Applied` or `Failed` + reason).
5. The API stores these into `device.observed_intent_version` and `device.network_conditions`. Updates are monotonic: a report is applied only if its `applied_version` is >= the stored value, so a delayed or duplicated report can never regress the sync state. A report carrying no version (old smithd, or a device that lost its local state) updates `network_conditions` (profile reality is still useful) but does not touch `observed_intent_version`; the sync chip therefore holds its last-derived state rather than regressing to "Unknown".
6. The dashboard derives sync state: `observed < intent` means "Applying...", equal with a `Failed` condition means "Error", equal and clean means "Synced".

"Is the device in sync?" is therefore a comparison of two integers plus a look at conditions, never a diff of network lists.

## Decisions

### D1. Intent and reality have disjoint writers `[settled unless objected]`

Operators and API endpoints write intent tables only; the device response path is the only writer of reality tables. Nothing ever merges the two.
*Rejected alternative:* a single "device networks" table with a status column per row. Every system that tried mixing the two (including our own `network_id`, which is silently both "what we want" and "what someone once set") ends up unable to say which side is wrong when they disagree.

### D2. Sync state is a version comparison, not a diff `[settled unless objected]`

`device.intent_version` bumps on any intent change; smithd persists and echoes the version of the last `ApplyNetworks` it executed; the report handler stores it as `observed_intent_version`.
*Rejected alternative:* comparing the intent list to the reported profile list field by field. With lists that third parties also mutate, a diff cannot distinguish "not yet applied" from "externally changed" from "applied but modified since", and every edge case becomes UI ambiguity. This is Kubernetes' `generation`/`observedGeneration` and Flight Control's renderedVersion, verbatim.

### D3. Intent is a ranked list, applied as one declarative command `[settled unless objected]`

A device's intent is a ranked *list* of networks (`device_network_intent`, N rows per device), replacing the single `network_id` FK. A single network cannot represent what devices need: a fallback network keeps the device reachable when the primary degrades, and NM autoconnect is built around choosing among multiple profiles anyway. Most devices already *have* multiple profiles in reality (BLE plus whatever accumulated); intent must be able to express that.
The list is unique per SSID: SSID is the reconciliation identity (D4), so the API rejects two intent entries with the same SSID for one device. A password rotation is therefore a PSK change on the existing entry, not a second entry; D5 defines how it applies without dropping connectivity.

`ApplyNetworks { version, networks: [{ssid, psk, priority}, ...] }` always ships the complete list. A device that missed three intent changes converges in one apply.
*Rejected alternative 1:* keep the single `network_id` push. Cannot express fallbacks or staged rotations, and forces every network change to be a hard cutover.
*Rejected alternative 2:* delta commands (AddNetwork/RemoveNetwork). Breaks the moment one message is missed or reordered; requires the server to track per-device command history to reconstruct state.

### D4. Reconcile by SSID, with adoption and a persisted last-applied list `[input wanted]`

Per intended SSID: modify an existing profile with that SSID in place (whoever created it, BLE included; for the profile currently carrying the connection, see D5), else create one. Delete only profiles that were in smithd's *previous applied list* and are absent from the new one (the list is persisted to disk, so it survives restarts; this is kubectl's last-applied-configuration three-way merge). Profiles whose SSID is outside the intent list (a tech's maintenance hotspot, customer equipment) are never modified, only reported. The two rules key on different things: intent membership (by SSID) is the sole trigger for touching a profile, regardless of who created it; the applied-record is the sole permission for deleting one. Wanting to modify an external profile and adding its SSID to intent are the same act; there is no other gateway.
Adoption is what fixes the stranded device: the wrong-PSK BLE profile has the intended SSID, so apply corrects it in place instead of racing a parallel profile for the same SSID (NetworkManager autoconnect picks one profile per SSID; leaving a broken higher-priority twin means flapping).
*Rejected alternative 1:* never touch profiles Smith didn't create. Makes the stranded-device case permanently unfixable remotely, which is the single most valuable case.
*Rejected alternative 2:* key reconciliation on NM profile *name* instead of SSID. Profile names are arbitrary local identity; SSID is what the network actually is, and same-SSID conflicts are the real hazard.
*Input wanted on:* the adoption lifecycle. Once adopted, a profile is smith-managed forever, including deletion when removed from intent. Half-management ("we fix your password but never remove you") seemed worse; disagree if you see a case.

**Priority semantics.** The intent list is ranked, and NetworkManager itself enforces the ranking: smithd writes `connection.autoconnect-priority` on each profile and never runs an imperative `nmcli connection up`. NM becomes the device-local reconciler, continuously picking the highest-priority visible network, including after reboots and signal changes (the D2/D3 level-based philosophy, one layer down). Single source of truth: the `priority` column in `device_network_intent`; the `ApplyNetworks` array is sorted by it, and smithd derives `autoconnect-priority = (list length - index) * 10` (the gap of 10 leaves room for manual tweaks during debugging).
An external profile with a hand-set higher `autoconnect-priority` still wins the connection, and Smith does not touch it: that shows up as drift via `current_network_id`, for the operator to resolve, not for the reconciler to fight.

### D5. Never break working connectivity `[settled unless objected]`

Apply never deletes or modifies the profile currently carrying the active connection until its replacement has successfully connected (create-connect-delete order). A failed apply must leave the device at least as connected as before.
This covers same-SSID password rotation: smithd creates a temporary second NM profile for the same SSID with the new PSK, connects it, and deletes the old profile only on success. On failure, smithd deletes the temporary profile before returning `Failed: WrongPSK` (D6), leaving the device exactly as connected as before and no stranded duplicate in NM. The temporary profile is never written to the applied-record; only the surviving profile is (the original on failure, the new one on success). Two profiles for one SSID exist only inside a single apply, never in intent (D3).

### D6. Failures are structured per-item conditions `[settled unless objected]`

The report payload gains two fields: `applied_version` (int) and `conditions` (array of `{ssid, state: Applied|Failed, reason: WrongPSK|NotInRange|NmcliError, message}`). Conditions describe the last apply attempt; current health is `current_network_id`. Both are additive: old smithd versions simply don't send them and show as "Unknown" sync state, so there is no coordinated deploy and no flag-day. `UpdateNetwork` dies by deprecation.
*Rejected alternative:* free-text error strings in logs (status quo). Requirement has always been that a wrong password is visible to the operator on the dashboard; that needs structure.

### D7. The catalog gets provenance and derived verification `[settled unless objected]`

The problem this solves: when an operator assigns a network to a device, the picker shows catalog rows that all look equally valid, but some were synced years ago and rotated since, some were typed by hand, and some were auto-created from device reports. The operator has no way to tell a working credential from a dead one until a device fails to connect. So the catalog gets two additions:

**Provenance** (where did this row come from): `source` (app-api | device-report | operator), `source_updated_at`, `created_at` on `network`. Cheap columns, no behavior change; they exist so the UI can say "synced from App API on 2026-03-01" instead of nothing.

**Verification** (do these credentials provably work): answered by looking at reality rather than by storing a fact. A device that is online right now with its active NM profile linked to catalog row X is living proof that X's credentials work, as fresh as that device's last heartbeat (1-20s). So the picker computes, per row: the most recent `last_seen` among online devices actively connected through that row (counting only devices whose report confirming the link is newer than the row's last edit). The result reads like "verified 30 seconds ago" for a network 200 devices sit on, and "last verified in March" for a backup SSID nothing currently uses (that fallback is a stored `verified_at`, written on exact ssid+psk match at report time and throttled to ~1 write/hour).
Verification must be *derived at read time*, not written on events. Reports are event-driven, so a perfectly healthy, stable fleet emits none; an event-written timestamp would make the healthiest network in the fleet look as stale as an abandoned one. Deriving from heartbeats means freshness decays only when devices actually stop being connected, which is exactly the signal an operator needs.
*Rejected alternative:* trying to make the catalog correct by construction. Its upstream can rotate a password an hour after a perfect sync; correctness is unknowable, trust level is not.

### D8. No silent credential auto-healing `[input wanted]`

The scenario: a device is connected and working on SSID X using password P1 (it got P1 via BLE at install, or the customer rotated and re-provisioned it). Meanwhile the catalog row for X, which this device's intent points at, still says P2. The device is holding fresher credentials than our catalog: reality is ahead of the record. Since #477 reports PSKs back, Smith can detect this exactly ("your working psk differs from the row your intent references") and could in principle fix the catalog automatically.

The decision is to surface it instead of fixing it silently: the network shows a "credential drift" warning with a one-click "update catalog from device", and an operator confirms. A human stays in the loop because of scoping: `network` rows are not tied to a site, and the same SSID can exist at two departments with different passwords ("Internal", "Guest"-style names especially). An automatic heal that copies device A's password into the row would silently break intent for site B's devices referencing the same row. The operator has the context ("yes, this is Solgården's rotation") that the code does not.

The cost: after a site-wide password rotation, someone clicks once per rotated network before re-applying to that site's devices. That is one click per rotation event, not per device, so the burden is small; if drift events in practice turn out to be unambiguous (single-department networks, all referencing devices agreeing), automating those specific cases later is easy because the detection machinery is identical.
*Input wanted on:* whether this should become automatic once drift events prove unambiguous in practice, and what evidence would justify it.

### D9. Reality-first UI with a single derived chip `[settled unless objected]`

One merged list (reality as the base), one chip: Unknown / Applying / Error / Synced, from D2's comparison plus D6's conditions. Intent appears only where it differs: ghost rows for pending adds, strikethrough for pending removals, badges for Active / External / failure reason. Rows matching intent carry no markers. Full intent-vs-reality diff is an on-demand detail, not permanent screen space. Pending state clears on version catch-up or timeout, never by optimistic guessing (the flip-flop lesson from Home Assistant).
The chip semantics follow from D2/D6; the presentation gets its own review on mockups when PR C is opened, not in this RFC.

### D10. Intent rows carry `managed_by` from day one `[input wanted]`

Every intent row records who set it: `operator:<user>` now, `app-api` reserved for phase 2. Until phase 2 this is just an audit trail costing one column.

In phase 2 the App API starts assigning department networks to devices automatically, which creates the possibility of conflict: the department default says network A, but an operator deliberately set device X to network B (a repeater, a wired-only corner, a debugging session). Proposed rule: **operator rows win on conflict, and the app-api writer may only add or remove rows it created**, so an automated sync can never silently undo a human's fix.

*Input wanted on two things:*
1. **The precedence rule.** Is "operator beats department default" right, or do you see cases where fleet uniformity should win over a manual override (e.g. an operator's forgotten debugging assignment blocking a site-wide migration)?
2. **How department defaults are stored.** Option (a): the App API writes plain rows into `device_network_intent` per device (simple, but 500 devices × 2 networks = 1000 rows that a sync job must keep consistent). Option (b): a department template stored once, with each device's effective intent computed as template + device overrides (the Flight Control fleet model: more machinery, no sync drift, scales to fleet-wide changes in one write). This RFC only commits to the `managed_by` column, which works under both; the a/b choice can wait for phase 2 but early opinions steer the schema.

## Open questions (answers needed, not just non-objection)

1. **Auto-apply timing.** Phase 0 is manual-only, which makes the historical "300 devices reconnect at once" fear moot (and prod says at most 139 devices even have legacy intent). When, if ever, do we let intent changes apply without a human click, and per device, per department, or globally?
2. **App API upsert sync.** The create-only `WHERE` clause is the root of the rotten-password story and of #481. The fix is small (also sync `network_wifis.updated_at > network_smith.updated_at`, plus an update call on Smith). Who takes it, and does anything block doing it before phase 2?
3. **Initial intent seeding.** Three options for what `device_network_intent` starts with:
   (a) migrate the 139 live devices' legacy `network_id` (tagged `managed_by: legacy`);
   (b) start empty, operators assign deliberately;
   (c) seed from reality: when a device's first profile report arrives (#477), adopt its working networks as its initial intent (tagged `managed_by: reality-seed`). Every device starts synced by construction, drift only appears when something actually changes, and BLE-provisioned networks become intent without an operator retyping them. Requires reality reporting deployed first and lands per device as reports come in, not in one migration.
   These compose: (c) for the fleet plus (a) where legacy intent exists but disagrees with reality (surfaces as "Out of date" for the operator to arbitrate). Which combination do we want?
4. **Department-to-device resolution for phase 2.** The App API owns this mapping natively: a device belongs to a department through its active bed assignment (`devices_bed_assignments → bed → department`); Smith's `department` label is a daily best-effort copy of that join (the labels sync job). Phase 2 should therefore drive assignment from the App API's own join, not from the Smith label. The open point: the join only covers bed-assigned devices, so department defaults are late-binding (a device gets its site's networks when it is assigned to a bed, not at first contact; BLE covers install time). Is late-binding acceptable, or do unassigned devices need a fallback?

## PR plan

Each PR is independently shippable, reviewable, and revertable. No pair requires a coordinated deploy.

| PR | Phase | Scope | Value shipped | Decisions / questions |
|----|-------|-------|---------------|-----------------------|
| A | 0 | Smith DB + API: `device_network_intent`, version columns, intent CRUD + Apply endpoint (bump version, queue command) | Intent becomes visible and auditable, before any device acts on it | D1, D2, D10 (column only); Q3 (seeding) |
| B | 0 | smithd: `ApplyNetworks` handler, SSID reconciliation, connectivity guard, persisted last-applied list, version + conditions in reports | The stranded-device fix; old API simply ignores the new fields | D2, D3, D4, D5, D6 |
| C | 0 | API report handler stores version/conditions; dashboard: intent section, chip, Apply button | Operators see truthful sync state; `UpdateNetwork` deprecated | D6, D9; Q1 (manual-only apply) |
| D | 1 | Catalog provenance columns + backfill, derived-verification query, credential-drift surface + "update from device", external-profile adopt/remove UI; network delete must list devices with intent referencing it (RESTRICT FK from PR A) | The bad catalog becomes visibly bad instead of silently wrong | D7, D8; #481 (must land first) |
| E | 2 | App API: upsert-capable sync (may land much earlier as a standalone fix); migrate `network.password` to a `credentials JSONB` column and update the App API sync to write into it; expand the `ApplyNetworks` credentials envelope beyond PSK | Password rotations propagate; #481 cannot recur; enterprise auth types become expressible | Q2; #488 |
| F | 2 | App API + Smith: department-default intent writer | New devices get their site's networks without an operator | D10; Q1 (auto-apply), Q4 |

Related but independent: #481 (catalog dedupe + sync idempotency) should land before D to avoid backfilling provenance onto ~590 rows that are about to be deleted.

### Phase 2 is directional, not specified

Phases 0 and 1 are specified to implementation level; phase 2 is a direction that gets its own RFC once Q1/Q4 are answered and phases 0-1 have run in prod. This RFC commits only to not blocking it (the `managed_by` column, D10). Known unknowns that the follow-up RFC must resolve:

- **What is a department default.** `network_wifis` has an `available` flag but no ranking; today's BLE broadcast takes the first available row (`LIMIT 1`, no `ORDER BY`). D4's priority semantics need a rank the App API data model cannot currently express.
- **Propagation semantics.** Staff adding a network to a department presumably bumps intent for every device in it; combined with auto-apply (Q1) that is a site-wide reconnect triggered from an app form. Removing one presumably deletes profiles from devices. Rotation edits (after PR E) propagate to the catalog, but who re-applies to affected devices?
- **Trigger.** Cron (like the existing sync and labels jobs) vs event-driven on `network_wifis` changes.
- **Scope.** WiFi only; the survey tables' ethernet and cellular data stay out.

## Review notes

**Enterprise WiFi auth (raised by @LudeeD)**

Some sites already use WPA-EAP / PEAP+MSCHAPv2 rather than plain PSK. The current credential shape (`{ssid, psk, priority}`) cannot express enterprise auth, which requires at minimum `key_mgmt`, `eap`, `phase2_auth`, and `identity` in addition to `password`.

Two things need to change before the relevant PRs ship:

- **Before PR B:** The `ApplyNetworks` payload shape (`{ssid, psk, priority}`) must become a credentials envelope so the protocol is not locked to PSK. Proposed: `{ssid, priority, credentials: {type: "psk", psk: "..."}}`, with `type: "eap"` carrying the additional fields. Changing this after PR B is in prod is a protocol break between smithd and the API.
- **Before PR E:** The `network` catalog currently stores only `password`. Migrating to a `credentials JSONB` column and expanding the `ApplyNetworks` envelope beyond PSK is PR E scope, not PR A or D, because the App API sync also writes to `network.password` — the two must move together. PR A produces `{"key_mgmt": "wpa-psk", "psk": <password>}` from `network.password` as an interim measure; the envelope shape is already extensible. PR E also needs to update the App API sync to write into `credentials` instead of `password`, and extend `NMProfile` reporting to include `key_mgmt` and EAP fields (see #488).

## Appendix

- Catalog duplication incident: #481
- Prior art: [Kubernetes API conventions (spec/status)](https://github.com/kubernetes/community/blob/main/contributors/devel/sig-architecture/api-conventions.md), [Server-Side Apply](https://kubernetes.io/docs/reference/using-api/server-side-apply/), [AWS IoT Device Shadow](https://docs.aws.amazon.com/iot/latest/developerguide/device-shadow-document.html), [Flight Control](https://github.com/flightctl/flightctl), [Argo CD diffing](https://argo-cd.readthedocs.io/en/stable/user-guide/diffing/), [Home Assistant on optimistic state](https://github.com/home-assistant/architecture/discussions/740)
