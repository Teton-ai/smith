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
	./scripts/ensure-device-jwt-key.sh
	test -f dashboard/.env || cp dashboard/.env.template dashboard/.env

gen-api-client:
	cd dashboard && npm run gen-api-client

seed:
	psql postgres://postgres:postgres@localhost:5432/postgres -f scripts/seed.sql

# Daemon↔API end-to-end tests. Run against the head stack with `make test.e2e`,
# or against the released API image with:
#   E2E_COMPOSE_FLAGS="-f compose.yaml -f compose.released-api.yaml" make test.e2e
# or run the released daemon against the head API with:
#   ./scripts/build-released-device.sh && E2E_UP_FLAGS=--no-build make test.e2e
E2E_COMPOSE_FLAGS ?= -f compose.yaml -f compose.e2e.yaml
E2E_SERVICES = postgres api bore device
# CI pre-builds images with buildx and sets this to --no-build.
E2E_UP_FLAGS ?= --build

test.e2e.up:
	./scripts/ensure-device-jwt-key.sh
	./scripts/ensure-e2e-auth0-issuer.sh
	DEVICE_BASE_IMAGE=ubuntu:22.04 docker compose $(E2E_COMPOSE_FLAGS) up -d $(E2E_UP_FLAGS) $(E2E_SERVICES)

test.e2e.run:
	./scripts/wait-for-api.sh
	cargo test --package smith-e2e -- --ignored --test-threads=1

test.e2e: test.e2e.up test.e2e.run

test.e2e.down:
	docker compose $(E2E_COMPOSE_FLAGS) down -v

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
