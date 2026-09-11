#!/usr/bin/env bash
# End-to-end check against a live MongoDB, one request at a time.
#
#   scripts/smoke.sh [connection-string]
#
# Creates a scratch database named mcpsmoke, exercises every tool against it,
# then drops it. Point it at a disposable server, never at production.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${MONGODB_MCP_BIN:-$ROOT/target/release/mongodb-mcp}"
URI="${1:-${MDB_MCP_CONNECTION_STRING:-mongodb://localhost:27017}}"
DB=mcpsmoke

if [[ ! -x "$BIN" ]]; then
  echo "binary not found at $BIN; run: cargo build --release" >&2
  exit 2
fi

echo "connecting to ${URI%%@*}${URI##*@} as $DB" >&2

STEPS=$(cat <<JSON
[
 {"name":"list-connections","args":{}},
 {"name":"list-databases","args":{}},
 {"name":"create-collection","args":{"database":"$DB","collection":"items"}},
 {"name":"insert-many","args":{"database":"$DB","collection":"items","documents":[{"name":"alpha","qty":3},{"name":"beta","qty":7},{"name":"gamma","qty":11,"tags":["x","y"],"meta":{"active":true}}]}},
 {"name":"find","args":{"database":"$DB","collection":"items","filter":{"qty":{"\$gt":5}},"sort":{"qty":-1}}},
 {"name":"count","args":{"database":"$DB","collection":"items","query":{}}},
 {"name":"aggregate","args":{"database":"$DB","collection":"items","pipeline":[{"\$group":{"_id":null,"total":{"\$sum":"\$qty"}}}]}},
 {"name":"update-many","args":{"database":"$DB","collection":"items","filter":{"name":"alpha"},"update":{"\$set":{"qty":99}}}},
 {"label":"update-many(replace guard)","name":"update-many","args":{"database":"$DB","collection":"items","filter":{},"update":{"qty":1}},"expectError":true},
 {"label":"delete-many(no filter)","name":"delete-many","args":{"database":"$DB","collection":"items"},"expectError":true},
 {"name":"create-index","args":{"database":"$DB","collection":"items","name":"qty_idx","definition":{"qty":-1}}},
 {"name":"collection-indexes","args":{"database":"$DB","collection":"items"}},
 {"name":"collection-schema","args":{"database":"$DB","collection":"items"}},
 {"name":"explain","args":{"database":"$DB","collection":"items","method":[{"name":"find","arguments":{"filter":{"qty":{"\$gt":5}}}}]}},
 {"name":"list-collections","args":{"database":"$DB"}},
 {"name":"db-stats","args":{"database":"$DB"}},
 {"name":"collection-storage-size","args":{"database":"$DB","collection":"items"}},
 {"label":"find(\$where guard)","name":"find","args":{"database":"$DB","collection":"items","filter":{"\$where":"this.qty > 1"}},"expectError":true},
 {"name":"export","args":{"database":"$DB","collection":"items","exportTitle":"t","exportTarget":{"filter":{}}}},
 {"name":"mongodb-logs","args":{"limit":2}},
 {"name":"delete-many","args":{"database":"$DB","collection":"items","filter":{"name":"beta"}}},
 {"name":"drop-index","args":{"database":"$DB","collection":"items","indexName":"qty_idx"}},
 {"name":"rename-collection","args":{"database":"$DB","collection":"items","newName":"things"}},
 {"name":"drop-collection","args":{"database":"$DB","collection":"things"}},
 {"name":"drop-database","args":{"database":"$DB"}},
 {"name":"disconnect","args":{}},
 {"label":"find(after disconnect)","name":"find","args":{"database":"$DB","collection":"items"},"expectError":true},
 {"name":"connect","args":{"connectionString":"$URI","connectionName":"reconnected"}}
]
JSON
)

MDB_MCP_CONNECTION_STRING="$URI" node "$ROOT/scripts/mcp-client.mjs" "$BIN" "$STEPS"
