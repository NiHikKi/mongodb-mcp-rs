//! Opening and closing MongoDB connections.

use serde_json::{Map, Value, json};

use super::{Context, ToolError, required_str};

pub async fn connect(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let uri = required_str(args, "connectionString")?;
    let name = args.get("connectionName").and_then(Value::as_str);
    let connection = ctx.connections.connect(&uri, name).await?;
    Ok(json!({
        "connectionId": connection.id,
        "name": connection.name,
        "connectionString": connection.redacted_uri,
        "message": "connected",
    }))
}

pub fn disconnect(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let id = args.get("connectionId").and_then(Value::as_str);
    let closed = ctx.connections.disconnect(id)?;
    Ok(json!({ "connectionId": closed, "message": "disconnected" }))
}
