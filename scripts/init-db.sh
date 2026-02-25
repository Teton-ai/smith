#!/bin/bash
set -e

export PGPASSWORD=$POSTGRES_PASSWORD

# Wait for the local PostgreSQL to be ready
until pg_isready; do
  echo "Waiting for PostgreSQL to start..."
  sleep 1
done

# Connect to external DB and dump the data using the database URL
pg_dump "$DEV_DATABASE_URL" \
  --exclude-table-data='telemetry*' \
  -Fd -j 1 -f /tmp/dump_dir -v

# Import the data into the local database
pg_restore --no-acl -U "$POSTGRES_USER" -d postgres -j 1 /tmp/dump_dir -v

echo "Database initialized with external data"
