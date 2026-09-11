---
title: Install the CLI
description: Install sm, the Smith command line tool.
---

## Download and install

Install `sm` with the install script. It downloads the latest release for your platform into `~/.smith/bin`.

```bash
curl -fsSL https://smith.teton.ai/install.sh | sh
```

The script needs `unzip`. Set `SMITH_CLI_INSTALL` to install somewhere other than `~/.smith`.

## Test your installation

```bash
sm --version
```

Use `sm help` to see help text documenting Smith's flags and usage, then [authenticate](./cli/overview.md#authentication).
