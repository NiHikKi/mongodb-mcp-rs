//! The tool catalogue, extracted from the reference server and embedded here.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::config::{CONFIRMATION_REQUIRED_TOOLS, Config};

const TOOLS_JSON: &str = include_str!("../data/tools.json");

#[derive(Debug, Deserialize)]
struct ToolsFile {
    tools: Vec<ToolMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    #[serde(rename = "operationType")]
    pub operation_type: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Map<String, Value>,
}

impl ToolMeta {
    /// Whether the tool only reads. Connection tools change session state but
    /// not data, so they stay available in read-only mode.
    pub fn is_read_only(&self) -> bool {
        matches!(self.operation_type.as_str(), "read" | "metadata" | "connect")
    }

    pub fn is_destructive(&self) -> bool {
        CONFIRMATION_REQUIRED_TOOLS.contains(&self.name.as_str())
    }
}

pub struct Registry {
    tools: Vec<ToolMeta>,
    read_only: bool,
    disabled: std::collections::BTreeSet<String>,
}

impl Registry {
    pub fn load(cfg: &Config) -> Result<Self> {
        let file: ToolsFile =
            serde_json::from_str(TOOLS_JSON).context("data/tools.json is malformed")?;
        Ok(Self {
            tools: file.tools,
            read_only: cfg.read_only,
            disabled: cfg.disabled_tools.clone(),
        })
    }

    pub fn get(&self, name: &str) -> Option<&ToolMeta> {
        self.tools.iter().find(|t| t.name == name)
    }

    pub fn visible(&self) -> Vec<&ToolMeta> {
        self.tools.iter().filter(|t| self.is_allowed(t)).collect()
    }

    pub fn is_allowed(&self, tool: &ToolMeta) -> bool {
        if self.disabled.contains(&tool.name) {
            return false;
        }
        !(self.read_only && !tool.is_read_only())
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn len(&self) -> usize {
        self.tools.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::from_env_and_args(Vec::<String>::new()).unwrap()
    }

    #[test]
    fn loads_the_whole_catalogue() {
        let reg = Registry::load(&config()).unwrap();
        assert_eq!(reg.len(), 25, "expected the v2 MongoDB tool set");
    }

    #[test]
    fn every_tool_has_a_description_and_schema() {
        let reg = Registry::load(&config()).unwrap();
        for t in &reg.tools {
            assert!(!t.description.is_empty(), "{} has no description", t.name);
            assert_eq!(
                t.input_schema.get("type").and_then(Value::as_str),
                Some("object"),
                "{} has no object schema",
                t.name
            );
        }
    }

    #[test]
    fn read_only_mode_hides_writes_but_keeps_reads() {
        let mut cfg = config();
        cfg.read_only = true;
        let reg = Registry::load(&cfg).unwrap();
        let names: Vec<&str> = reg.visible().iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"find"));
        assert!(names.contains(&"connect"), "connecting is not a data change");
        assert!(!names.contains(&"insert-many"));
        assert!(!names.contains(&"drop-database"));
    }

    #[test]
    fn disabled_tools_are_removed() {
        let mut cfg = config();
        cfg.disabled_tools = ["drop-database".to_string()].into_iter().collect();
        let reg = Registry::load(&cfg).unwrap();
        assert!(!reg.visible().iter().any(|t| t.name == "drop-database"));
    }

    #[test]
    fn destructive_tools_are_marked() {
        let reg = Registry::load(&config()).unwrap();
        assert!(reg.get("drop-database").unwrap().is_destructive());
        assert!(reg.get("delete-many").unwrap().is_destructive());
        assert!(!reg.get("find").unwrap().is_destructive());
    }

    #[test]
    fn every_catalogued_tool_is_dispatchable() {
        // Names the executor does not know would be listed but always fail.
        const KNOWN: &[&str] = &[
            "connect", "disconnect", "list-connections", "find", "aggregate", "aggregate-db",
            "count", "explain", "export", "list-databases", "list-collections",
            "collection-indexes", "collection-schema", "collection-storage-size", "db-stats",
            "mongodb-logs", "insert-many", "update-many", "delete-many", "create-collection",
            "create-index", "rename-collection", "drop-collection", "drop-database", "drop-index",
        ];
        let reg = Registry::load(&config()).unwrap();
        for t in &reg.tools {
            assert!(KNOWN.contains(&t.name.as_str()), "{} has no handler", t.name);
        }
        assert_eq!(KNOWN.len(), reg.len());
    }
}
