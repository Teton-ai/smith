# Getting Started

Follow this guide to get started with local development.

## Setup

First, you will need the following stuff installed/setup:
- Docker & Docker Compose
- Rust (1.75+)  
- Node.js (18+)
- PostgreSQL client (`psql`)
- 1Password CLI (optional)

Then run `make init`. This will initialize the repo with all the stuff you need. This will:
- Create service-specific .env files (`api/.env` and `dashboard/.env`) from templates
- You'll need to fill in the missing values in both files

## Starting the platform

Run `make up`, this should start the api, a local replica of smithd running on a device, and a bore server.

Try running

```sh
curl http://localhost:8080/health
```

to check that it's running as it should. It should return something like

```
I'm good: <version number>
```

## Starting the dashboard

Run the following
```
cd dashboard
npm i
npm run dev
```

The dashboard should now be running on localhost:3000

You can now open the dashboard, and see a device in the "Pending Approval" box. Press approve, and you should be golden.

You will probably need to `make seed` here, or preferrably earlier, so the approval will work.

## Starting the CLI

Run
```sh
cargo run --bin sm -- <your command>
```
You probably need to run it some times initially since it needs to create some defaults.
If it shows a list of commands, you are golden.
