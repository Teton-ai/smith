# Smith (Agent Smith) ![GitHub release (latest SemVer)](https://img.shields.io/github/v/release/teton-ai/smith?sort=semver)

<img src="https://docs.smith.teton.ai/logo.png" width="150" style="border-radius: 12%;">

**Smith**, also known as Agent Smith, is Teton's Fleet Management System. It provides automation to easily deploy and monitor applications at scale.

## Architecture

Smith consists of three main components:

- **Smith API**: Backend service that manages deployment configurations and fleet status.
- **SmithD**: Daemon process that runs on each node in the fleet to execute deployments and report status back to the API.
- **Dashboard**: The Smith Dashboard, a visual interface to see the current status of the fleet.
- **Smith CLI (sm)** Command Line Interface to interact with the Smith API as a Fleet administrator.

## LICENSE

The Smith source and documentation are released under the [Apache License 2.0](./LICENSE)
