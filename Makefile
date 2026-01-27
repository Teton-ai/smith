include .env
export DOCKER_CLI_HINTS=false

.DEFAULT_GOAL := up

# Starts the platform locally
# To start multiple devices, run `docker compose up --scale device=3 -d`
up:
	docker compose up -d

migrate:
	cd api && DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres" cargo sqlx migrate run

prepare:
	cd api && DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres" cargo sqlx prepare

dev.docs:
	cd docs && mdbook serve --open

lint:
	docker exec -it smith-api cargo fmt
	docker exec -it smith-api cargo clippy --release --all-targets --all-features -- -D clippy::all
	cd dashboard && npm run lint

fix:
	cargo fix --allow-dirty --allow-staged

schema:
	docker exec smith-postgres pg_dump --schema-only -n public -U $(POSTGRES_USER) postgres > schema.sql

init:
	echo "Initializing the repo"
	test -f .env || cp .env.template .env
	test -f dashboard/.env || cp dashboard/.env.template dashboard/.env

gen-api-client:
	cd dashboard && npm run gen-api-client

seed:
	psql postgres://postgres:postgres@localhost:5432/postgres -f seed.sql

debug.smithd:
	cargo build --release -p smith
    sudo ln -sf $(CURDIR)/target/release/smithd /usr/bin/smithd
