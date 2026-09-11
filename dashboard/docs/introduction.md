---
title: Introduction
description: Smith is an open-source fleet management system for deploying, monitoring and updating devices at scale.
---

Smith is built in Rust and was born from managing thousands of devices across healthcare facilities. It gives you the automation and reliability you need to deploy, monitor and update your fleet with confidence.

## Why Smith?

- **Built for scale** — manage anything from hundreds to thousands of devices.
- **Reliable** — designed for >99% uptime in critical environments.
- **Seamless updates** — deploy upgrades and rollbacks with no manual intervention.
- **Transparent** — open-source infrastructure you can trust and extend.

## Architecture

Smith consists of five components:

| Component | What it does |
|---|---|
| `smithd` | Daemon that runs on each device to execute deployments and report status |
| `updater` | Daemon that keeps `smithd` up to date |
| `api` | Backend service managing deployment configurations and fleet status |
| `dashboard` | Visual interface to monitor your fleet in real time, and home of these docs |
| `sm` | Command line tool for fleet administrators |

## Where to next

- [Install the CLI](./installation.md)
- [Run Smith locally](./development.md)
- [Roll out a release](./deployments.md)
- [Browse the API reference](/docs/api)

## Security

If you discover a security vulnerability, please email **security@teton.ai**. We operate a bug bounty program and have paid bounties for responsibly disclosed vulnerabilities. See the [security policy](https://github.com/Teton-ai/smith/blob/main/SECURITY.md) for details.

## License

Smith and its documentation are released under the [Apache License 2.0](https://github.com/Teton-ai/smith/blob/main/LICENSE).
