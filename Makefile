.PHONY: db-up db-down api worker fmt lint test check

db-up:
	docker compose up -d db

db-down:
	docker compose down

api:
	cargo run --bin api

worker:
	cargo run --bin worker

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

check: fmt lint test
