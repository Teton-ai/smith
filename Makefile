export DOCKER_CLI_HINTS=false

.DEFAULT_GOAL := dev

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
