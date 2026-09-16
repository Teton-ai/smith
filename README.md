# Smith (Agent Smith) ![GitHub release (latest SemVer)](https://img.shields.io/github/v/release/teton-ai/smith?sort=semver)

<p align="center">
  <img src="https://cdn.prod.website-files.com/65d7113b5391824218ef5c3a/68de33d3d5eb5159c0a4be35_oss_smith%202.png" alt="Smith Fleet Management" width="600">
</p>

**Smith** is an open-source fleet management system built in Rust for managing distributed IoT devices at scale. Born from managing thousands of devices across healthcare facilities, Smith provides the automation and reliability you need to deploy, monitor, and update your fleet with confidence.

## Why Smith?

- **Built for Scale**: Manage everything from hundreds to thousands of devices
- **Reliable**: Designed for >99% uptime in critical environments
- **Seamless Updates**: Deploy upgrades and rollbacks with zero manual intervention
- **Transparent**: Open-source infrastructure you can trust and extend

## Architecture

Smith consists of five main components:

- **smithd**: Daemon that runs on each device to execute deployments and report status
- **updater**: Daemon that keeps smithd up to date
- **api**: Backend service managing deployment configurations and fleet status
- **dashboard**: Visual interface to monitor your fleet in real-time
- **cli (sm)**: Command-line tool for fleet administrators

## Getting Started

Read the [documentation](./dashboard/docs/introduction.md) to get started with Smith. The dashboard also serves it at `/docs`, together with an API reference generated from your API.

### Externally managed devices (NixOS)

Set `externally_managed = true` in the existing `[meta]` section of the device's
`magic.toml`, then restart `smithd`. The setting defaults to `false`.

```toml
[meta]
magic_version = 2
server = "https://api.smith.teton.ai/smith"
externally_managed = true
```

This keeps registration, authentication, secrets delivery to
`/root/.teton_environment`, NetworkManager provisioning, monitoring, logs,
read-only file browsing and tunnels. It disables Debian package updates
(including Smith self-updates), NVIDIA OTA, remote reboot and arbitrary shell
commands, SSH configuration hardening, and the connectivity reboot watchdog.
The local control API rejects package updates, OTA and downloads to the device
with HTTP 403; rejected remote commands return a failure response.

NixOS must provide SSH hardening, packages and service lifecycle management.
Do not install the separate `smith-updater` service. Keep `magic.toml` writable
and persistent: Smith stores its registration token there. It loads
`./magic.toml` before `/etc/smith/magic.toml`. Keep secrets outside the Nix store;
arrange application startup after initial secrets delivery and restart applications
when their environment changes.

This is an operational policy, not a security sandbox: secrets, network profiles
and tunnel SSH keys remain mutable, and tunnel users retain their normal SSH
permissions. NetworkManager profiles should be owned by Smith rather than also
managed declaratively by NixOS.

## Local Development

**Prerequisites:** Docker

```bash
make init       # creates .env and dashboard/.env from templates
make up         # starts all services (api, dashboard, postgres, bore, device)
make migrate    # runs database migrations
make seed       # seeds the database with test data
```

- API: `http://localhost:8080`
- Dashboard: `http://localhost:3000`

### Device options

Set in `.env` or inline with `docker compose up`:

| Variable | Default | Description |
|---|---|---|
| `DEVICE_BASE_IMAGE` | `nvcr.io/nvidia/l4t-base:r36.2.0` | Base image. Use `ubuntu:22.04` on x86_64. |
| `DEVICE_REPLICAS` | `1` | Number of simulated devices |
| `NETWORK_THROTTLE` | `random` | `none`, `random`, or a fixed Mbps value |
| `GLOBAL_BANDWIDTH_LIMIT` | `100` | API egress cap in Mbps |

```bash
DEVICE_BASE_IMAGE=ubuntu:22.04 DEVICE_REPLICAS=3 NETWORK_THROTTLE=none docker compose up
```

## Contributing

We welcome contributions! Whether you're fixing bugs, adding features, or improving documentation, your help makes Smith better for everyone. Check out our issues or submit a PR.

## Security

Security is a top priority for Smith. If you discover a security vulnerability, please email **security@teton.ai**. We operate a bug bounty program and have paid bounties for responsibly disclosed vulnerabilities. See our [Security Policy](./SECURITY.md) for more details.

## License

The Smith source and documentation are released under the [Apache License 2.0](./LICENSE)
