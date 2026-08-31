# Deployments

Deployments are how you roll out new releases to your fleet. Smith uses a canary-first approach: you select a small set of devices, verify they're healthy, then confirm a full rollout to all remaining devices in the distribution.

## Core Concepts

- **Package list** — a named, versioned set of packages that defines what software runs on your devices.
- **Release** — a published snapshot of a package list, immutable once created. Every deployment targets a specific release.
- **Distribution** — a logical group of devices that shares a target release. Devices belong to exactly one distribution.
- **Canary devices** — the small subset of devices that receive a release first, before the rest of the fleet.
- **Release candidate (RC)** — a release flagged so it can never roll out fleet-wide. It stays on its canary devices permanently unless explicitly promoted to a stable release.
- **LTS** — a release designated as one to keep devices on long-term. The flag is a record of that decision, so every version ever designated keeps its mark and the history stays readable.
- **Follow latest** — a per-device flag deciding whether fleet-wide rollouts move that device. Devices that do not follow latest stay where they are until an operator targets them explicitly.

---

## How a Deployment Works

1. **Create a release** from a package list.
2. **Create a deployment** targeting that release. Smith assigns a set of canary devices.
3. **Canary devices update** to the new release.
4. **Verify health** — check that canary devices are running correctly (services healthy, online, no regressions).
5. **Confirm full rollout** — the remaining devices in the distribution update. Or, if something looks wrong, yank the release before it spreads.

---

## Walkthrough: RC to Full Rollout

This example walks through a complete release cycle using a release candidate for early validation, then promoting it to a stable release and rolling it out automatically to the whole fleet.

### Step 1 — Create a new package list revision

Open the **Package Lists** section in the dashboard and update your package list with the new set of packages. Save the revision — this becomes the source of truth for what will be installed on devices.

### Step 2 — Publish a release candidate

From the package list, publish a new release and mark it as a **release candidate**. Give it a meaningful name, such as `v1.5.0-rc.1`.

Because it is an RC, Smith will never automatically roll it out beyond its canary devices — this flag is a hard guardrail.

### Step 3 — Deploy the RC to a fixed set of devices

Create a deployment for the RC release and target a specific set of devices by their IDs. For this example, choose three devices from a known-good lab bench:

- `device-lab-01`
- `device-lab-02`
- `device-lab-03`

These devices will receive the RC build. The rest of the fleet is unaffected.

### Step 4 — Validate the RC

Monitor the canary devices in the dashboard. Look for:

- All watchdog-monitored services reporting healthy.
- Devices staying online (last ping within the expected window).
- No unexpected reboots or error logs.

Let the RC run on these devices for as long as your process requires — hours, days, or longer. There is no time pressure because a full rollout is blocked by design.

### Step 5 — Promote the RC to a stable release

Once you are satisfied with the RC, promote it to a stable release from the **Releases** page. This removes the RC flag and makes the release eligible for fleet-wide rollout.

The devices already running the RC continue running it uninterrupted — promotion does not trigger any updates on its own.

### Step 6 — Create a new deployment with automatic canary selection

Now publish a new deployment targeting the promoted stable release, this time using **automatic canary selection**. Smith will pick up to 10 canary devices from the distribution, prioritizing:

1. Devices with all watchdog services healthy.
2. Devices with higher network speed test scores.
3. Devices with the most recent last ping.

This means the healthiest, best-connected devices in your fleet receive the update first — maximizing the chance of catching issues before they reach everything else.

### Step 7 — Confirm full rollout

After verifying the automatic canary devices are healthy, confirm the full rollout from the deployment detail page. All remaining devices in the distribution will update to the new release.

---

## Selection Strategies

When creating a deployment, you can choose how canary devices are selected:

- **Automatic** (recommended) — Smith picks up to 10 devices based on health, network quality, and recency. Best for most deployments.
- **Label-based** — targets devices matching one or more labels, useful for deploying to a specific site or hardware variant first.
- **Explicit device IDs** — hand-pick specific devices, useful for targeting a known lab bench or QA set.

---

## Release Candidates

The `release_candidate` flag blocks `confirm_full_rollout` entirely. It is not a soft warning — there is no override. Use it when:

- A build should be validated on a small set before being eligible for fleet-wide distribution.
- You are running long-duration A/B tests on specific hardware.
- An experimental build should never reach production automatically.

To reach the full fleet, an RC must be explicitly promoted to a stable release first.

---

## Device Eligibility

A device is eligible for *automatic* canary selection when it is:

- **Online** — last ping within 3 minutes.
- **Up-to-date** — currently running its target release (not mid-update).
- **In the same distribution** — belongs to the distribution the release targets.
- **Following latest** — `follow_latest` is true.

Explicit selection by device id or label ignores `follow_latest`: naming a device
by hand is taken as deliberate, which is what lets a pinned lab bench still be
used as a canary.

---

## LTS Releases

Marking a release LTS records that it is a version you commit to keeping devices
on — the build a device is flashed with in the factory, so you can tell later
what shipped on which version.

```
POST /releases/{id}   { "lts": true }
sm releases lts <release_id>            # or --clear to remove the mark
```

A release cannot be marked LTS while it is a draft, yanked, or a release
candidate: a draft was never published, a yank withdrew it, and an RC is by
definition not fleet-wide material.

The flag is kept on every release that ever had it, so the list of LTS versions
is a history rather than a single pointer. To resolve which one is currently in
force — what a flashing process should install — ask for the distribution's
current LTS release, which returns the most recently designated one that is
still published and not withdrawn:

```
GET /distributions/{distribution_id}/releases/lts
```

---

## Pinning Devices Off Rollouts

`confirm_full_rollout` retargets every device in the distribution **except**
those with `follow_latest = false`. This is a hard skip, not a warning: a
fleet-wide rollout cannot move a pinned device.

New devices are pinned by default, so a freshly flashed device stays on the
release it shipped with until someone decides otherwise. Devices that existed
before the flag was introduced were backfilled as following latest, so the
existing fleet's behaviour is unchanged.

Move devices between the two states in bulk, either by id or by label:

```
PUT /devices/follow-latest
{ "follow_latest": true, "devices": [1, 2, 3] }
{ "follow_latest": false, "labels": ["site=oslo", "stage=field"] }
```

Exactly one of `devices` or `labels` must be given. A device must carry **every**
label listed to match — unlike canary label selection, which matches any. The
narrower rule is deliberate: this endpoint decides whether a device receives
rollouts at all, so a selector should never match more than intended. Archived
devices are excluded from label matching. The response reports how many devices
changed.

The device listing accepts `follow_latest` as a filter, so you can see what is
pinned before and after a bulk move.

---

## Best Practices

- Use RCs for any build that needs validation before reaching the full fleet — the hard rollout block removes the risk of accidentally confirming too early.
- Run network speed tests on your devices so Smith has connectivity data to factor into automatic selection.
- Check the deploy modal before confirming — it shows per-device network quality and service health so you can spot outliers.
- Monitor canary devices for a meaningful window before confirming, not just the first few minutes after the update lands.
- Check what is pinned (`GET /devices?follow_latest=false`) before a large rollout, so the set of devices staying behind is one you chose rather than one you inherited.
