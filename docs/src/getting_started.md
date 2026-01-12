# Getting Started

Follow this guide to get started with local development.

## Setup

First, you will need the following stuff installed/setup:
- Docker
- 1password
- node

Then run `make init`. This will initialize the repo with all the stuff you need. At the time of writing this will
- Create .env files (.env and dashboard/.env) for you. Please fill in the missing values

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

## Starting the CLI

Run
```sh
cargo run --bin sm -- <your command>
```
You probably need to run it some times initially since it needs to create some defaults.
If it shows a list of commands, you are golden.
