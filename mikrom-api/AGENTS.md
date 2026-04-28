# AGENTS.md

## Commands

```bash
# Run the API server
cargo run

# Run all tests (requires docker-compose up)
cargo nextest run

# Run a single test
cargo nextest run test_name

# Run unit tests only
cargo nextest run --lib

# Run integration tests
cargo nextest run --test integration
```

## Prerequisites

- **PostgreSQL must be running**: `docker-compose up -d postgres`
- Tests connect to `TEST_DATABASE_URL` or default to `postgres://mikrom:mikrom_password@localhost:5432/mikrom_api_test`
- Set `JWT_SECRET` before running tests (tests set it via `env::set_var`)

## Architecture

- **Entry**: `src/main.rs` → runs on `http://172.16.0.13:5001`
- **Routes**: `/health`, `/auth/register`, `/auth/login`
- **State**: `AppState { db: sqlx::PgPool }` in `src/lib.rs`
