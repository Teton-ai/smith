---
title: Local development
description: Run the API, dashboard, database and simulated devices on your machine.
---

## Prerequisites

- Docker
- Node.js, for working on the dashboard
- Rust, for working on the CLI and daemons

## Start the platform

```bash
make init       # creates .env and dashboard/.env from templates
make up         # starts all services (api, dashboard, postgres, bore, device)
make migrate    # runs database migrations
make seed       # seeds the database with test data
```

Fill in any missing values in the `.env` files `make init` created.

- API: `http://localhost:8080`
- Dashboard: `http://localhost:3000`

Check that the API is running:

```bash
curl http://localhost:8080/health
```

It should answer with its version:

```text
I'm good: <version number>
```

Open the dashboard and you'll see the simulated device waiting in **Pending Approval**. Approve it and it starts reporting in. Approval needs the seed data, so run `make seed` first.

## Device options

Set these in `.env` or inline with `docker compose up`:

| Variable | Default | Description |
|---|---|---|
| `DEVICE_BASE_IMAGE` | `nvcr.io/nvidia/l4t-base:r36.2.0` | Base image. Use `ubuntu:22.04` on x86_64. |
| `DEVICE_REPLICAS` | `1` | Number of simulated devices |
| `NETWORK_THROTTLE` | `random` | `none`, `random`, or a fixed Mbps value |
| `GLOBAL_BANDWIDTH_LIMIT` | `100` | API egress cap in Mbps |

```bash
DEVICE_BASE_IMAGE=ubuntu:22.04 DEVICE_REPLICAS=3 NETWORK_THROTTLE=none docker compose up
```

## Run the CLI from source

```bash
cargo run --bin sm -- <your command>
```

The first runs create a default configuration. Once it prints the list of commands, you're set.

## Write docs

These pages are markdown files in `dashboard/docs`, served by the dashboard at `/docs`. Each file starts with frontmatter:

```yaml
---
title: Page title
description: One sentence shown under the title.
---
```

A file's path is its URL: `dashboard/docs/cli/get.md` is `/docs/cli/get`. Add new pages to the sidebar in `dashboard/app/docs/content.ts`, and link between pages with relative `.md` paths so they also work on GitHub.

The [API reference](/docs/api) is generated from the API's `/openapi.json`, so endpoints are documented in their `utoipa` annotations and doc comments, not here.
