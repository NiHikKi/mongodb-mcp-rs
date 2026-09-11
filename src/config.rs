//! Runtime configuration.
//!
//! Environment names mirror the reference server (`MDB_MCP_*`), so an existing
//! MCP configuration keeps working after swapping the binary.

use std::collections::BTreeSet;

use anyhow::Result;

/// Byte ceiling on a single query response, matching the reference default.
pub const DEFAULT_MAX_BYTES_PER_QUERY: usize = 16 * 1024 * 1024;
/// Document ceiling on a single query response.
pub const DEFAULT_MAX_DOCUMENTS_PER_QUERY: usize = 100;

/// Operators that run JavaScript inside the server; refused unless re-enabled.
pub const SERVER_SIDE_JS_OPERATORS: &[&str] = &["$where", "$function", "$accumulator"];

/// Tools the reference asks a human to confirm before running.
pub const CONFIRMATION_REQUIRED_TOOLS: &[&str] =
    &["drop-database", "drop-collection", "delete-many", "drop-index"];

#[derive(Debug, Clone)]
pub struct Config {
    /// Connection opened at startup, so `connect` is not needed first.
    pub connection_string: Option<String>,
    /// Refuse every tool that writes.
    pub read_only: bool,
    /// Refuse queries that would scan a collection.
    pub index_check: bool,
    pub disable_server_side_js: bool,
    pub max_bytes_per_query: usize,
    pub max_documents_per_query: usize,
    pub max_active_connections: usize,
    /// Server-side time limit applied to every operation.
    pub max_time_ms: Option<u64>,
    pub disabled_tools: BTreeSet<String>,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    match env_opt(key) {
        None => default,
        Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    env_opt(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_list(key: &str) -> BTreeSet<String> {
    env_opt(key)
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

impl Config {
    /// The first non-flag argument is a connection string, as in the reference.
    pub fn from_env_and_args<I: IntoIterator<Item = String>>(args: I) -> Result<Self> {
        let mut positional = None;
        let mut read_only_flag = false;
        let mut it = args.into_iter().peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--read-only" | "--readOnly" => read_only_flag = true,
                "--connectionString" | "--connection-string" => positional = it.next(),
                a if a.starts_with("--connectionString=") => {
                    positional = a.split_once('=').map(|(_, v)| v.to_string());
                }
                a if a.starts_with("--") => {}
                a => {
                    if positional.is_none() {
                        positional = Some(a.to_string());
                    }
                }
            }
        }

        Ok(Self {
            connection_string: positional.or_else(|| env_opt("MDB_MCP_CONNECTION_STRING")),
            read_only: read_only_flag || env_bool("MDB_MCP_READ_ONLY", false),
            index_check: env_bool("MDB_MCP_INDEX_CHECK", false),
            disable_server_side_js: env_bool("MDB_MCP_DISABLE_SERVER_SIDE_JS", true),
            max_bytes_per_query: env_usize(
                "MDB_MCP_MAX_BYTES_PER_QUERY",
                DEFAULT_MAX_BYTES_PER_QUERY,
            ),
            max_documents_per_query: env_usize(
                "MDB_MCP_MAX_DOCUMENTS_PER_QUERY",
                DEFAULT_MAX_DOCUMENTS_PER_QUERY,
            ),
            max_active_connections: env_usize("MDB_MCP_MAX_ACTIVE_CONNECTIONS", 10),
            max_time_ms: env_opt("MDB_MCP_MAX_TIME_MS").and_then(|v| v.parse().ok()),
            disabled_tools: env_list("MDB_MCP_DISABLED_TOOLS"),
        })
    }
}

/// Replace the credentials in a connection string with `*****`.
///
/// Connection strings reach logs, tool output and error messages, so this runs
/// on every path that shows one.
pub fn redact_connection_string(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_string();
    };
    // Credentials live before the last `@` of the authority section.
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let Some((_credentials, host)) = authority.rsplit_once('@') else {
        return uri.to_string();
    };
    format!("{scheme}://*****:*****@{host}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_username_and_password() {
        let out = redact_connection_string("mongodb://alice:s3cret@db.example.com:27017/admin");
        assert_eq!(out, "mongodb://*****:*****@db.example.com:27017/admin");
        assert!(!out.contains("s3cret"));
        assert!(!out.contains("alice"));
    }

    #[test]
    fn redacts_srv_uris_with_query_options() {
        let out = redact_connection_string(
            "mongodb+srv://user:p%40ss@cluster0.abcd.mongodb.net/?retryWrites=true",
        );
        assert!(!out.contains("p%40ss"));
        assert!(out.contains("cluster0.abcd.mongodb.net"));
        assert!(out.contains("retryWrites=true"));
    }

    #[test]
    fn leaves_credential_free_uris_alone() {
        let uri = "mongodb://localhost:27017";
        assert_eq!(redact_connection_string(uri), uri);
    }

    #[test]
    fn leaves_a_password_containing_an_at_sign_fully_hidden() {
        let out = redact_connection_string("mongodb://u:pa@ss@host:27017/db");
        assert!(!out.contains("pa@ss"));
        assert!(out.starts_with("mongodb://*****:*****@host:27017/db"));
    }

    #[test]
    fn parses_a_positional_connection_string() {
        let cfg = Config::from_env_and_args(["mongodb://localhost:27017".to_string()]).unwrap();
        assert_eq!(cfg.connection_string.as_deref(), Some("mongodb://localhost:27017"));
    }

    #[test]
    fn read_only_flag_is_honoured() {
        let cfg = Config::from_env_and_args(["--read-only".to_string()]).unwrap();
        assert!(cfg.read_only);
    }
}
