# AGENTS.md

## Commands

```bash
# Run the API server
cargo run

# Run all tests (requires docker-compose up)
cargo test

# Run a single test
cargo test test_name

# Run unit tests only
cargo test --lib

# Run integration tests
cargo test --test integration
```

## Prerequisites

- **PostgreSQL must be running**: `docker-compose up -d postgres`
- Tests connect to `TEST_DATABASE_URL` or default to `postgres://mikrom:mikrom_password@localhost:5432/mikrom_api`
- Set `JWT_SECRET` before running tests (tests set it via `env::set_var`)

## Architecture

- **Entry**: `src/main.rs` → runs on `http://172.16.0.13:5001`
- **Routes**: `/health`, `/auth/register`, `/auth/login`
- **State**: `AppState { db: sqlx::PgPool }` in `src/lib.rs`
