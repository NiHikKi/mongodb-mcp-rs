# mongodb-mcp

A MongoDB [Model Context Protocol](https://modelcontextprotocol.io) server written in
Rust, built on the official MongoDB Rust driver. It reimplements the 25 database
tools of `mongodb-mcp-server` v2 with the same names and argument schemas.

## Why

The Node server runs as two processes and holds roughly 100 MB per Claude
session. This one is a single static binary that starts in milliseconds.

## Tools

All 25 database tools of the reference server:

| Group | Tools |
| --- | --- |
| Connections | `connect`, `disconnect`, `list-connections` |
| Reads | `find`, `aggregate`, `aggregate-db`, `count`, `explain`, `export` |
| Metadata | `list-databases`, `list-collections`, `collection-indexes`, `collection-schema`, `collection-storage-size`, `db-stats`, `mongodb-logs` |
| Writes | `insert-many`, `update-many`, `delete-many` |
| Structure | `create-collection`, `create-index`, `rename-collection`, `drop-collection`, `drop-database`, `drop-index` |

Tool names, descriptions and JSON schemas are extracted from the reference
server and embedded in the binary, so a client sees the same interface.

Several connections can be open at once. Tools take an optional `connectionId`;
when only one connection is open the argument may be omitted.

## Safety

The defaults follow the reference server, and the checks are enforced before a
command reaches the database:

- **Server-side JavaScript is refused.** `$where`, `$function` and `$accumulator`
  are rejected at any depth of a filter or pipeline.
- **`update-many` requires update operators.** A plain document would replace
  every matched record rather than modify it, so it is refused.
- **`delete-many` requires an explicit filter.** Emptying a collection has to be
  asked for with `{}`, never by omission.
- **Read-only mode** hides and refuses every write, including an aggregation
  whose pipeline ends in `$out` or `$merge`.
- **Credentials never leave the process.** Connection strings are redacted
  wherever they appear in output, logs or errors.
- **Database content is fenced.** Documents come back wrapped in a boundary that
  tells the model to treat them as data, not as instructions.

## Configuration

Environment names match the reference server, so an existing MCP configuration
keeps working. The first positional argument is also read as a connection string.

| Variable | Default | Meaning |
| --- | --- | --- |
| `MDB_MCP_CONNECTION_STRING` | none | Connection opened at startup. |
| `MDB_MCP_READ_ONLY` | false | Refuse every write. |
| `MDB_MCP_INDEX_CHECK` | false | Refuse queries answered by a collection scan. |
| `MDB_MCP_DISABLE_SERVER_SIDE_JS` | true | Refuse `$where`, `$function`, `$accumulator`. |
| `MDB_MCP_MAX_DOCUMENTS_PER_QUERY` | 100 | Document ceiling per response. |
| `MDB_MCP_MAX_BYTES_PER_QUERY` | 16777216 | Byte ceiling per response. |
| `MDB_MCP_MAX_ACTIVE_CONNECTIONS` | 10 | Connections held at once. |
| `MDB_MCP_MAX_TIME_MS` | none | Server-side time limit per operation. |
| `MDB_MCP_DISABLED_TOOLS` | empty | Comma-separated tool names to hide. |
| `MDB_MCP_LOG` | warn | Log filter for stderr. |

Example configuration:

```json
{
  "mcpServers": {
    "mongodb": {
      "command": "/path/to/mongodb-mcp",
      "env": {
        "MDB_MCP_CONNECTION_STRING": "${MONGODB_URI}",
        "MDB_MCP_READ_ONLY": "true"
      }
    }
  }
}
```

## Build and verify

```sh
cargo build --release
cargo test
scripts/smoke.sh mongodb://localhost:27017
```

`scripts/smoke.sh` drives the server over stdio one request at a time, creating
a scratch database, exercising every tool against it and dropping it afterwards.
It also checks that each guard refuses what it should. Point it at a disposable
server, never at production.

## Differences from the reference server

- Only the stdio transport is implemented. The reference also offers HTTP.
- Atlas cloud, Atlas local deployment and knowledge-base tools are not included;
  this server covers the database tools only.
- `export` returns documents inline, capped like any other read, rather than
  writing a file and handing back a URI.
- `collection-schema` infers field types and their frequency from a sample. The
  reference uses the `mongodb-schema` library and reports a richer structure.
- Destructive tools are marked with MCP annotations rather than prompting for
  confirmation through elicitation.

## Licence

MIT, see `LICENSE`. Parts of what ships here are derived from other projects and
carry their licences; `ATTRIBUTION.md` says exactly which parts and under which
terms.
