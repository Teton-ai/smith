# Smith (Agent Smith) ![GitHub release (latest SemVer)](https://img.shields.io/github/v/release/teton-ai/smith?sort=semver)

<p align="center">
  <img src="https://www.teton.ai/_next/image?url=%2F_next%2Fstatic%2Fmedia%2Fsmith.a4a7eb54.png&w=1080&q=75" alt="Smith Fleet Management" width="600">
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

## License

The Smith source and documentation are released under the [Apache License 2.0](./LICENSE)
