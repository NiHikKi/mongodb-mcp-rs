# Attribution

This project is an independent reimplementation, but part of what ships in it is
derived from another project and carries that project's licence.

## mongodb-mcp-server

Upstream: https://github.com/mongodb-js/mongodb-mcp-server — Apache Licence 2.0,
Copyright MongoDB, Inc.

`data/tools.json` holds the 25 tool names, descriptions and JSON input schemas
extracted from that package (version 2.1.1), so that a client sees the same
interface. About 2 KB of the file is upstream description text.

Changes made relative to upstream, as Apache 2.0 section 4(b) requires them to be
stated:

- The server is written in Rust on the official MongoDB Rust driver; none of the
  upstream implementation is reused.
- Only the stdio transport is implemented; the HTTP transport is absent.
- Atlas cloud, Atlas local deployment and knowledge-base tools are not included.
- `export` returns documents inline rather than writing a file and returning a URI.
- `collection-schema` infers field types and frequencies from a sample instead of
  using the `mongodb-schema` library.
- Destructive tools are marked with MCP annotations instead of prompting through
  elicitation.

A copy of the Apache Licence 2.0 is at https://www.apache.org/licenses/LICENSE-2.0
and applies to the derived tool metadata described above. Everything under `src/`
is original work licensed under the MIT terms in `LICENSE`.

This project is not affiliated with MongoDB, Inc.
