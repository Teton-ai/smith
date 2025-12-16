include .env
export DOCKER_CLI_HINTS=false

.DEFAULT_GOAL := up

up:
	docker compose up

dev:
	docker exec -it smith-smithd cargo run --bin api

migrate:
	docker exec -it smith-smithd sh -c "cd api && cargo sqlx migrate run"

prepare:
	docker exec -it smith-smithd  sh -c "cd api && cargo sqlx prepare"

dev.docs:
	cd docs && mdbook serve --open

lint:
	docker exec -it smith-smithd cargo fmt
	docker exec -it smith-smithd cargo clippy --release --all-targets --all-features -- -D clippy::all

fix:
	docker exec -it smith-smithd cargo fix --allow-dirty --allow-staged

run:
	docker exec -it smith-smithd cargo run --bin smithd

schema:
	docker exec smith-postgres pg_dump --schema-only -n public -U $(POSTGRES_USER) postgres > schema.sql

init:
	echo "Initializing the repo"
	test -f .env || cp .env.template .env
	test -f dashboard/.env || cp dashboard/.env.template dashboard/.env

gen-api-client:
	cd dashboard && npm run gen-api-client && cd ..
