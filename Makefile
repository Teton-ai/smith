include .env
export DOCKER_CLI_HINTS=false

.DEFAULT_GOAL := up

up:
	docker compose up

migrate:
	cd api && DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres" cargo sqlx migrate run

prepare:
	cd api && DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres" cargo sqlx prepare

dev.docs:
	cd docs && mdbook serve --open

lint:
	cargo fmt
	cargo clippy --release --all-targets --all-features -- -D clippy::all
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
	# Add dummy package
	curl -X PUT 'http://localhost:8080/packages' -F file=@dummy-package_1.0.0_amd64.AppImage
