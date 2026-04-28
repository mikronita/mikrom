#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE DATABASE mikrom_api;
    CREATE DATABASE mikrom_api_test;
    CREATE DATABASE mikrom_scheduler;
    CREATE DATABASE mikrom_scheduler_test;
    CREATE DATABASE mikrom_router;
    CREATE DATABASE mikrom_router_test;
    GRANT ALL PRIVILEGES ON DATABASE mikrom_api TO $POSTGRES_USER;
    GRANT ALL PRIVILEGES ON DATABASE mikrom_api_test TO $POSTGRES_USER;
    GRANT ALL PRIVILEGES ON DATABASE mikrom_scheduler TO $POSTGRES_USER;
    GRANT ALL PRIVILEGES ON DATABASE mikrom_scheduler_test TO $POSTGRES_USER;
    GRANT ALL PRIVILEGES ON DATABASE mikrom_router TO $POSTGRES_USER;
    GRANT ALL PRIVILEGES ON DATABASE mikrom_router_test TO $POSTGRES_USER;
EOSQL
