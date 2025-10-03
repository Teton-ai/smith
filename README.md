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

Visit our [documentation](https://docs.smith.teton.ai) to get started with Smith.

## Contributing

We welcome contributions! Whether you're fixing bugs, adding features, or improving documentation, your help makes Smith better for everyone. Check out our issues or submit a PR.

## Security

Security is a top priority for Smith. If you discover a security vulnerability, please email **security@teton.ai**. We operate a bug bounty program and have paid bounties for responsibly disclosed vulnerabilities. See our [Security Policy](./SECURITY.md) for more details.

## License

The Smith source and documentation are released under the [Apache License 2.0](./LICENSE)
