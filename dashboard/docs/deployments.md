---
title: Deployments
description: Roll out new releases canary-first, verify they're healthy, then confirm across the distribution.
---

Deployments are how you roll out new releases to your fleet. A small set of canary devices updates first; once they are healthy you confirm, and the rest of the distribution updates in staggered waves. The **Deployments** page shows every rollout: how far it has got, who started it, and what it changes.

## Core concepts

- **Package list** — a named, versioned set of packages that defines what software runs on your devices.
- **Release** — a published snapshot of a package list, immutable once created. Every deployment targets a specific release.
- **Distribution** — a logical group of devices that shares a target release. Devices belong to exactly one distribution.
- **Canary devices** — the small subset of devices that receive a release first, before the rest of the fleet.
- **Release candidate (RC)** — a release flagged so it can never roll out fleet-wide. It rolls out to a **pilot group** you pick by labels or by hand when you deploy it: canaries first, then the rest of the group. What the fleet follows doesn't change.
- **LTS** — a release designated as one to keep devices on long-term. The flag is a record of that decision, so every version ever designated keeps its mark and the history stays readable.
- **Follow latest** — a per-device flag deciding whether fleet-wide rollouts move that device. Devices that do not follow latest stay where they are until an operator targets them explicitly.

## How a deployment works

1. **Create a release** from a package list.
2. **Create a deployment** targeting that release. Smith picks the canary devices (see [Device eligibility](#device-eligibility)).
3. **Canary devices update** to the new release.
4. **Verify health** — check that canary devices are running correctly (services healthy, online, no regressions). Every canary has to finish, and at least one has to be running the release.
5. **Confirm the rollout** — the remaining devices in the distribution update in staggered waves (see [Staggered rollouts](#staggered-rollouts)). If something looks wrong, [pause or yank](#controlling-a-deployment) it.

Each deployment is its own record with its own history, so deploying a release again adds a new one. Fleet deployments run one at a time per distribution, because a second one to confirm would undo the first. Release candidates roll out to pilot groups rather than the fleet, so they don't count and can run alongside a fleet deployment.

## Walkthrough: RC to full rollout

This example walks through a complete release cycle using a release candidate for early validation on a pilot group, then promoting it to a stable release and rolling it out to the whole fleet.

### Step 1 — Create a new package list revision

Open the **Package Lists** section in the dashboard and update your package list with the new set of packages. Save the revision — this becomes the source of truth for what will be installed on devices.

### Step 2 — Publish a release candidate

From the package list, publish a new release and mark it as a **release candidate**. Give it a meaningful name, such as `v1.5.0-rc.1`.

Because it is an RC, Smith will never roll it out beyond the pilot group you choose — this flag is a hard guardrail.

### Step 3 — Deploy the RC to a pilot group

Deploy the RC and choose its pilot group **By Labels** (for example `env=staging`) or by hand with **Select Devices**. Smith picks up to 10 of its online devices as the canaries; a group under 10 devices updates directly instead.

### Step 4 — Validate the RC

Monitor the canary devices in the dashboard. Look for:

- All watchdog-monitored services reporting healthy.
- Devices staying online (last ping within the expected window).
- No unexpected reboots or error logs.

Once the canaries are healthy, **Confirm rollout to pilot group** updates the rest of the group in staggered waves; labels are matched again then, so newly labelled devices join.

### Step 5 — Promote the RC to a stable release

Once you are satisfied with the RC, promote it to a stable release from the **Releases** page. This removes the RC flag and makes the release eligible for fleet-wide rollout.

The devices already running the RC continue running it uninterrupted — promotion does not trigger any updates on its own.

### Step 6 — Deploy the stable release

Now deploy the promoted stable release. Smith picks up to 10 canary devices from the distribution, prioritizing:

1. Devices with all watchdog services healthy.
2. Devices with higher network speed test scores.
3. Devices with the most recent last ping.

This means the healthiest, best-connected devices in your fleet receive the update first — maximizing the chance of catching issues before they reach everything else.

### Step 7 — Confirm full rollout

After verifying the automatic canary devices are healthy, confirm the rollout from the deployment page. All remaining devices in the distribution update to the new release, in staggered waves over up to 95 minutes, and the page follows them wave by wave.

## Who can ship what

Users keep the roles defined in `api/roles.toml`; `ACCOUNTS_CONFIG` assigns each user a role, with `default` for anyone not listed. **Settings > Roles** shows permissions and inheritance.

Publishing releases and controlling deployments require `devices:write`, granted to both `default` and `admin`. Package previews show upgrades, downgrades, additions, and removals before a deployment starts.

To roll back a package, draft a new release from latest and select its older version, or redeploy an earlier release after reviewing all package changes.

## Controlling a deployment

Open a deployment to control it.

- **Pause** — no more devices start updating, even in waves already open. A device already downloading finishes and stays there.
- **Resume** — waves still to come keep their spacing, pushed back by the pause, which is also added to the rollout's timeout.
- **Yank** — withdraw the release and move every device on it, or heading to it, back to the release the deployment started from, at once. It is how a rollout is stopped, running or finished, so no device that follows latest is left off latest. The move is recorded as a rollback deployment, which can't itself be paused or yanked.
- **Finalize now** — once every wave has opened, stop waiting and complete the deployment now instead of at its timeout (see below).
- **Rush rollout** — every device whose wave hasn't opened yet moves into one new wave, right after the last opened one, that opens immediately. Waves that already opened keep their devices and times. It is for emergencies: a site whose devices all download together can lose connectivity.

All of this appears in the deployment's activity log and the deployment Slack channel.

## Device states

| State | Meaning |
| --- | --- |
| Updated | Reached the release. |
| Pending | Allowed to move and online, but not updated yet. |
| Waiting | Its wave hasn't opened yet. It stays on its current release. |
| Held | Its deployment is paused. |
| Offline | Allowed to move, but hasn't been seen for 3 minutes. It updates when it returns. |
| Moved elsewhere | Someone or something retargeted it since. The latest action wins. |

A wave is **Failing** only when updated devices have failing watchdog services; offline or pending devices don't fail it, since in a large fleet some always are.

A rollout completes when nothing is waiting or pending, or at its timeout: twice its stagger window after the first wave opened, at least an hour. Offline devices never hold it open. Devices that haven't updated by then are **Timed out**, but keep their target and update on their own later. The result is then frozen; later check-ins don't change it.

## Staggered rollouts

Confirming a rollout does not move every device at once. When a whole site downloads a release together, its network can saturate and devices drop offline, so the devices are split into up to 20 waves that open 5 minutes apart: a full fleet rollout takes about 95 minutes from confirm to the last wave. Smaller sets get one device per wave.

- **Who goes first** is pseudo-random, derived from the device and the release, so it changes with every release and never follows device ids (a site's devices are provisioned in a row) or network quality.
- **Waves open on time only.** Nothing halts automatically and no wave waits for the previous one to finish. The canary is the risk gate; pausing is how you stop a bad rollout.

### Deploy to Selected

Deploying a release directly to selected devices from the device list follows the same rule above 15 devices: the selection is staggered over up to 95 minutes, and the deploy dialog says so before you submit. **Deploy all at once** skips the stagger after the same emergency confirmation. Selections of 15 devices or fewer update right away. The CLI uses the same default.

It is recorded as a deployment with no canary phase, so you can follow, pause and yank it the same way. It doesn't change what the fleet follows and can run alongside other deployments.

Single-device target changes are never staggered, and they aren't recorded as deployments.

## Release candidates

The `release_candidate` flag keeps a release off the fleet: it can only roll out to the pilot group chosen when it was deployed, and never becomes what the distribution follows. It is not a soft warning — there is no override. Use it when:

- A build should be validated on a small set before being eligible for fleet-wide distribution.
- You are running long-duration A/B tests on specific hardware.
- An experimental build should never reach production automatically.

To reach the full fleet, an RC must be explicitly promoted to a stable release first.

A fleet rollout leaves devices on a newer release candidate where they are: shipping hotfix `1.4.1` doesn't pull a pilot group off `1.5.0-rc`. They move once a stable release at least as new as their RC rolls out.

## Device eligibility

A device can be a canary when it is:

- **Online** — last ping within 3 minutes.
- **Up-to-date** — currently running its target release (not mid-update).
- **In the same distribution** — belongs to the distribution the release targets.
- **Following latest** — `follow_latest` is true, for a stable release. A release candidate's canaries come from its pilot group instead, pinned or not: choosing the group is deliberate.
- **Not on a newer release candidate**, for a stable release.

## LTS releases

Marking a release LTS records that it is a version you commit to keeping devices on — the build a device is flashed with in the factory, so you can tell later what shipped on which version.

```text
POST /releases/{id}   { "lts": true }
sm releases lts <release_id>            # or --clear to remove the mark
```

A release cannot be marked LTS while it is a draft, yanked, or a release candidate: a draft was never published, a yank withdrew it, and an RC is by definition not fleet-wide material.

The flag is kept on every release that ever had it, so the list of LTS versions is a history rather than a single pointer. To resolve which one is currently in force — what a flashing process should install — ask for the distribution's current LTS release, which returns the most recently designated one that is still published and not withdrawn:

```text
GET /distributions/{distribution_id}/releases/lts
```

## Pinning devices off rollouts

Confirming a rollout retargets every device in the distribution **except** those with `follow_latest = false`. This is a hard skip, not a warning: a fleet-wide rollout cannot move a pinned device.

New devices are pinned by default, so a freshly flashed device stays on the release it shipped with until someone decides otherwise. Devices that existed before the flag was introduced were backfilled as following latest, so the existing fleet's behaviour is unchanged.

Move devices between the two states in bulk, either by id or by label:

```text
PUT /devices/follow-latest
{ "follow_latest": true, "devices": [1, 2, 3] }
{ "follow_latest": false, "labels": ["site=oslo", "stage=field"] }
```

Exactly one of `devices` or `labels` must be given. A device must carry **every** label listed to match — unlike a pilot group's labels, which match any. The narrower rule is deliberate: this endpoint decides whether a device receives rollouts at all, so a selector should never match more than intended. Archived devices are excluded from label matching. The response reports how many devices changed.

The device listing accepts `follow_latest` as a filter, so you can see what is pinned before and after a bulk move.

The CLI wraps this by device id: `sm pin` / `sm unpin` take the usual device selection (ids, `-l`, `-s`), and `sm pin --release <id>` sets the target release before pinning. `sm get d --pinned` lists pinned devices.

## Best practices

- Use RCs for any build that needs validation before reaching the full fleet — the hard rollout block removes the risk of accidentally confirming too early.
- Run network speed tests on your devices so Smith has connectivity data to factor into automatic selection.
- Monitor canary devices for a meaningful window before confirming, not just the first few minutes after the update lands.
- Check what is pinned (`GET /devices?follow_latest=false`) before a large rollout, so the set of devices staying behind is one you chose rather than one you inherited.
