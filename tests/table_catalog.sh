#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

exec python3 -B tests/mysql_acceptance.py -- sh -eu -c '
    mysql() {
        docker exec "$OPENRUST_ACCEPTANCE_MYSQL_CONTAINER" sh -ec \
            '"'"'MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql --protocol=TCP -h 127.0.0.1 \
            -uroot "$MYSQL_DATABASE" -Nse "$1"'"'"' table-catalog "$1"
    }

    cargo +1.95.0 test -p openrust-framework --test database_schema --locked --offline \
        mysql_inspects_visible_schemas_tables_and_views_on_the_write_connection -- --exact
    mysql "CREATE DATABASE openrust_wipe;
           CREATE TABLE catalog_scope_sentinel (id INT PRIMARY KEY);
           INSERT INTO catalog_scope_sentinel VALUES (1)"
    export OPENRUST_TEST_MYSQL_WIPE_URL="${OPENRUST_TEST_MYSQL_URL%/*}/openrust_wipe"
    cargo +1.95.0 test --test migrations --locked --offline mysql_fresh_wipes_the_selected_schema -- --exact
    test "$(mysql "SELECT COUNT(*) FROM catalog_scope_sentinel")" = 1
'
