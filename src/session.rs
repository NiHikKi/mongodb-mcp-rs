//! The set of live MongoDB connections a session holds.
//!
//! Tools address a connection by `connectionId`; when only one is open, that
//! argument may be omitted.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use mongodb::Client;
use mongodb::options::ClientOptions;
use serde_json::{Value, json};

use crate::config::{Config, redact_connection_string};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SessionError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, SessionError> {
    Err(SessionError(msg.into()))
}

pub struct Connection {
    pub id: String,
    pub name: String,
    pub client: Client,
    /// Safe to show: the credentials are already replaced.
    pub redacted_uri: String,
}

pub struct Connections {
    /// Insertion ordered; the first entry is the implicit default.
    inner: RwLock<Vec<Arc<Connection>>>,
    max_active: usize,
}

impl Connections {
    pub fn new(cfg: &Config) -> Self {
        Self { inner: RwLock::new(Vec::new()), max_active: cfg.max_active_connections }
    }

    /// Open a connection and verify it answers before keeping it.
    pub async fn connect(
        &self,
        uri: &str,
        name: Option<&str>,
    ) -> Result<Arc<Connection>, SessionError> {
        {
            let open = self.inner.read().expect("connection lock poisoned");
            if open.len() >= self.max_active {
                return err(format!(
                    "the maximum of {} active connections is already open; disconnect one first",
                    self.max_active
                ));
            }
        }

        let redacted_uri = redact_connection_string(uri);
        let mut options = ClientOptions::parse(uri)
            .await
            .map_err(|e| SessionError(format!("{redacted_uri} is not a usable connection string: {e}")))?;
        options.app_name = Some("mongodb-mcp-rs".to_string());
        options.server_selection_timeout = Some(Duration::from_secs(10));

        let client = Client::with_options(options)
            .map_err(|e| SessionError(format!("could not build a client for {redacted_uri}: {e}")))?;

        // A connection that cannot answer a ping is not worth keeping.
        client
            .database("admin")
            .run_command(bson::doc! { "ping": 1 })
            .await
            .map_err(|e| SessionError(format!("could not reach {redacted_uri}: {e}")))?;

        let id = uuid::Uuid::new_v4().to_string();
        let connection = Arc::new(Connection {
            name: name.map(str::to_owned).unwrap_or_else(|| redacted_uri.clone()),
            id: id.clone(),
            client,
            redacted_uri,
        });
        self.inner.write().expect("connection lock poisoned").push(connection.clone());
        Ok(connection)
    }

    /// Look a connection up, defaulting to the only open one.
    pub fn resolve(&self, id: Option<&str>) -> Result<Arc<Connection>, SessionError> {
        let open = self.inner.read().expect("connection lock poisoned");
        match id {
            Some(wanted) => open
                .iter()
                .find(|c| c.id == wanted || c.name == wanted)
                .cloned()
                .ok_or_else(|| SessionError(format!("no open connection with id {wanted:?}"))),
            None => match open.len() {
                0 => err(
                    "not connected to MongoDB; call connect with a connection string first",
                ),
                1 => Ok(open[0].clone()),
                _ => err(
                    "several connections are open; pass connectionId to say which one to use",
                ),
            },
        }
    }

    pub fn disconnect(&self, id: Option<&str>) -> Result<String, SessionError> {
        let target = self.resolve(id)?;
        let mut open = self.inner.write().expect("connection lock poisoned");
        open.retain(|c| c.id != target.id);
        Ok(target.id.clone())
    }

    pub fn list(&self) -> Value {
        let open = self.inner.read().expect("connection lock poisoned");
        let items: Vec<Value> = open
            .iter()
            .map(|c| {
                json!({
                    "connectionId": c.id,
                    "name": c.name,
                    "connectionString": c.redacted_uri,
                })
            })
            .collect();
        json!({ "connections": items, "count": items.len() })
    }

}
