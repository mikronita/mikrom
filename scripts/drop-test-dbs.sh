#!/bin/bash
set -e

# Configuration - use environment variables if set, otherwise defaults
DB_USER="${POSTGRES_USER:-mikrom}"
DB_PASSWORD="${POSTGRES_PASSWORD:-mikrom_password}"
DB_HOST="${POSTGRES_HOST:-localhost}"
DB_PORT="${POSTGRES_PORT:-5432}"
MAINTENANCE_DB="postgres"

export PGPASSWORD="$DB_PASSWORD"

echo "Searching for test databases matching 'mikrom_%_test%'..."

# Get list of databases to drop
DBS_TO_DROP=$(psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$MAINTENANCE_DB" -t -c "SELECT datname FROM pg_database WHERE datname LIKE 'mikrom_%_test%';" | xargs)

if [ -z "$DBS_TO_DROP" ]; then
    echo "No test databases found to drop."
    exit 0
fi

echo "Found the following test databases to drop:"
for db in $DBS_TO_DROP; do
    echo " - $db"
done

# Confirmation (optional, but let's make it non-interactive for automation)
# To make it interactive, we could add a check here.

for db in $DBS_TO_DROP; do
    echo "Dropping database: $db..."
    
    # Terminate active connections
    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$MAINTENANCE_DB" -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '$db' AND pid <> pg_backend_pid();" > /dev/null
    
    # Drop the database
    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$MAINTENANCE_DB" -c "DROP DATABASE IF EXISTS \"$db\";"
    
    echo "Successfully dropped $db."
done

echo "Done! All test databases have been dropped."
