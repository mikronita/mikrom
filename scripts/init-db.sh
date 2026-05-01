#!/bin/bash
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    SELECT 'CREATE DATABASE mikrom_api' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mikrom_api')\gexec
    SELECT 'CREATE DATABASE mikrom_api_test' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mikrom_api_test')\gexec
    SELECT 'CREATE DATABASE mikrom_scheduler' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mikrom_scheduler')\gexec
    SELECT 'CREATE DATABASE mikrom_scheduler_test' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mikrom_scheduler_test')\gexec
    SELECT 'CREATE DATABASE mikrom_router' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mikrom_router')\gexec
    SELECT 'CREATE DATABASE mikrom_router_test' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'mikrom_router_test')\gexec
EOSQL
