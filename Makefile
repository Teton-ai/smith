export DOCKER_CLI_HINTS=false

# Default values that can be overridden by environment variables or .env file
POSTGRES_USER ?= postgres

# Optionally include .env file if it exists (but don't fail if it doesn't)
-include .env

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
	psql postgres://postgres:postgres@localhost:5432/postgres -f scripts/seed.sql

debug.smithd:
	cargo build --release -p smith
	sudo ln -sf $(CURDIR)/target/release/smithd /usr/bin/smithd

DEVICES ?= $(shell docker ps --filter "name=smith-device" --format "{{.Names}}")

watch.smithd:
	cargo watch -s "make deploy.smithd" -w smithd -w models

deploy.smithd:
	docker compose up -d smithd-builder
	docker exec smith-smithd-builder cargo build --package smith --bin smithd
	docker cp smith-smithd-builder:/app/target/debug/smithd /tmp/smithd-deploy
	@pids=""; status=0; \
	for device in $(DEVICES); do \
		(echo "Deploying to $$device..." && \
		docker cp /tmp/smithd-deploy $$device:/usr/bin/smithd && \
		docker exec $$device systemctl restart smithd) & \
		pids="$$pids $$!"; \
	done; \
	for pid in $$pids; do wait $$pid || status=1; done; \
	exit $$status
