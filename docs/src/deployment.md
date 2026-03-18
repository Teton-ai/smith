# Deployments

Deployments are how you roll out new releases to your fleet. Smith uses a canary-first approach: you select a small set of devices, verify they're healthy, then confirm a full rollout to all devices in the distribution.

## How Deployments Work

1. **Select canary devices** — pick devices to receive the update first (automatic, by labels, or by device ID)
2. **Canary update** — selected devices update to the new release
3. **Verify health** — check that canary devices are running correctly (services healthy, devices online)
4. **Confirm full rollout** — push the update to all remaining devices in the distribution

If canary devices have problems, you can yank the release instead of confirming rollout.

## Selection Strategies

### Automatic (recommended)

Let Smith pick up to 10 canary devices. Devices are prioritized by:

1. **Healthy services first** — devices with all watchdog services running are preferred
2. **Better network first** — devices with higher network speed test scores are preferred
3. **Recently seen first** — devices with a more recent last ping are preferred

This is the best option for most deployments.

```sh
smith deploy create --release-id 42
```

### Label-based

Target devices matching specific labels. Useful when you want to deploy to a specific subset, like a particular site or hardware variant.

```sh
smith deploy create --release-id 42 --labels "site=warehouse-a"
```

### Explicit device IDs

Hand-pick specific devices. Useful for testing on known-good hardware.

```sh
smith deploy create --release-id 42 --device-ids 101,102,103
```

## Release Candidates

Mark a release as a **release candidate** to deploy it to a fixed set of canary devices without ever rolling out fleet-wide. The `release_candidate` flag blocks `confirm_full_rollout`, so the release stays on its canary devices only.

Use release candidates for:
- Experimental builds that should never reach the whole fleet
- Long-running validation on a subset of devices
- A/B testing with specific hardware

## Device Eligibility

A device is eligible for canary selection when it is:

- **Online** — last ping within 3 minutes
- **Up-to-date** — currently running its target release (not mid-update)
- **In the same distribution** — belongs to the distribution the release targets

## Best Practices

- **Use automatic selection** for most deployments — Smith picks the healthiest, best-connected devices
- **Use release candidates** for experimental builds that should never roll out fleet-wide
- **Monitor canary service health** before confirming full rollout — the dashboard shows per-service status for canary devices
- **Run network speed tests** on devices so Smith can factor connectivity into automatic selection
- **Check the deploy modal** before confirming — it shows network quality and service health indicators for each device
