# Getting Started

Follow this guide to get started with local development.

## Setup

First, you will need the following stuff installed/setup:
- Docker
- 1password
- rust
- node

Then, create a `.env` file in the root, fill in these values

```.env
POSTGRES_USER=postgres
POSTGRES_PASSWORD=postgres
DATABASE_URL="postgres://postgres:postgres@postgres:5432/postgres"

PACKAGES_BUCKET_NAME=# Get this from your coworker
ASSETS_BUCKET_NAME=# Get this from your coworker
AWS_REGION=eu-north-1

AUTH0_ISSUER=# Get this from your coworker
AUTH0_AUDIENCE=# Get this from your coworker
```

Then create a `dashboard/.env` file, and fill it with these values
```.env
API_BASE_URL=http://localhost:8080
AUTH0_DOMAIN=# Get this from your coworker
AUTH0_CLIENT_ID=# Get this from your coworker
AUTH0_REDIRECT_URI=http://localhost:3000
AUTH0_AUDIENCE=# Get this from your coworker
```

## Starting the api

Run `make up`, then `make migrate`
and finally `make dev` to start the api.

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

## Starting smithd

First, create a magic.toml in the root with the following content
```toml
[meta]
magic_version = 2
server = "http://127.0.0.1:8080/smith"
token = "<GENERATE A RANDOM UUID>"
```

Then run `make run`, which will start smithd.

You can now open the dashboard, and see a device in the "Pending Approval" box.

Now open your favorite database editor, open the database, and then type

```psql
UPDATE device SET approved = 't', token = '<SAME UUID AS ABOVE>';
```

Going back in to the dashboard should show the device as registered.
