//! Metadata: databases, collections, indexes, schema and statistics.

use std::collections::BTreeMap;

use bson::{Bson, Document};
use futures::StreamExt;
use serde_json::{Map, Value, json};

use super::{Context, ToolError, db_and_collection, required_str, untrusted};

pub async fn list_databases(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let conn = ctx.connection(args)?;
    let dbs = conn.client.list_databases().await?;
    let items: Vec<Value> = dbs
        .into_iter()
        .map(|d| json!({ "name": d.name, "sizeOnDisk": d.size_on_disk, "empty": d.empty }))
        .collect();
    Ok(json!({ "databases": items, "count": items.len() }))
}

pub async fn list_collections(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let database = required_str(args, "database")?;
    let conn = ctx.connection(args)?;
    let mut cursor = conn.client.database(&database).list_collections().await?;
    let mut items = Vec::new();
    while let Some(spec) = cursor.next().await {
        let spec = spec?;
        items.push(json!({ "name": spec.name, "type": format!("{:?}", spec.collection_type) }));
    }
    Ok(json!({ "database": database, "collections": items, "count": items.len() }))
}

pub async fn collection_indexes(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    let mut cursor = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .list_indexes()
        .await?;
    let mut items = Vec::new();
    while let Some(index) = cursor.next().await {
        let index = index?;
        items.push(
            bson::serialize_to_bson(&index)
                .map(Bson::into_relaxed_extjson)
                .unwrap_or_else(|e| json!({ "error": e.to_string() })),
        );
    }
    Ok(json!({
        "database": database,
        "collection": collection,
        "indexes": items,
        "count": items.len(),
    }))
}

/// Infer a schema by sampling documents and recording the types each field takes.
pub async fn collection_schema(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    let sample_size = args.get("sampleSize").and_then(Value::as_u64).unwrap_or(100).clamp(1, 1000);

    let mut cursor = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .aggregate(vec![bson::doc! { "$sample": { "size": sample_size as i64 } }])
        .await?;

    let mut fields: BTreeMap<String, FieldStats> = BTreeMap::new();
    let mut sampled = 0u64;
    while let Some(doc) = cursor.next().await {
        let doc = doc?;
        sampled += 1;
        record_fields(&doc, "", &mut fields);
    }

    let schema: Vec<Value> = fields
        .into_iter()
        .map(|(path, stats)| {
            json!({
                "field": path,
                "types": stats.types,
                "count": stats.count,
                "probability": if sampled == 0 { 0.0 } else { stats.count as f64 / sampled as f64 },
            })
        })
        .collect();

    Ok(untrusted(
        &format!("schema of {database}.{collection} inferred from {sampled} sampled documents"),
        &json!({ "sampledDocuments": sampled, "fields": schema }),
    ))
}

#[derive(Default)]
struct FieldStats {
    count: u64,
    types: Vec<String>,
}

fn record_fields(doc: &Document, prefix: &str, out: &mut BTreeMap<String, FieldStats>) {
    for (key, value) in doc {
        let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        let entry = out.entry(path.clone()).or_default();
        entry.count += 1;
        let type_name = bson_type_name(value);
        if !entry.types.iter().any(|t| t == type_name) {
            entry.types.push(type_name.to_string());
        }
        // Nested documents get their own paths so the caller can see the shape.
        if let Bson::Document(inner) = value {
            record_fields(inner, &path, out);
        }
        if let Bson::Array(items) = value
            && let Some(Bson::Document(inner)) = items.first()
        {
            record_fields(inner, &format!("{path}[]"), out);
        }
    }
}

fn bson_type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "object",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => "javascript",
        Bson::Int32(_) => "int",
        Bson::Int64(_) => "long",
        Bson::Timestamp(_) => "timestamp",
        Bson::Binary(_) => "binary",
        Bson::ObjectId(_) => "objectId",
        Bson::DateTime(_) => "date",
        Bson::Decimal128(_) => "decimal",
        Bson::Undefined => "undefined",
        Bson::MaxKey => "maxKey",
        Bson::MinKey => "minKey",
        Bson::DbPointer(_) => "dbPointer",
        Bson::Symbol(_) => "symbol",
    }
}

pub async fn collection_storage_size(
    ctx: &Context,
    args: &Map<String, Value>,
) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    let stats = conn
        .client
        .database(&database)
        .run_command(bson::doc! { "collStats": &collection })
        .await?;
    let pick = |key: &str| stats.get(key).cloned().map(Bson::into_relaxed_extjson);
    Ok(json!({
        "database": database,
        "collection": collection,
        "storageSize": pick("storageSize"),
        "size": pick("size"),
        "totalIndexSize": pick("totalIndexSize"),
        "count": pick("count"),
    }))
}

pub async fn db_stats(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let database = required_str(args, "database")?;
    let conn = ctx.connection(args)?;
    let stats = conn
        .client
        .database(&database)
        .run_command(bson::doc! { "dbStats": 1 })
        .await?;
    Ok(Bson::Document(stats).into_relaxed_extjson())
}

pub async fn logs(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let conn = ctx.connection(args)?;
    let kind = args.get("type").and_then(Value::as_str).unwrap_or("global");
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;

    let out = conn
        .client
        .database("admin")
        .run_command(bson::doc! { "getLog": kind })
        .await?;

    let all: Vec<Value> = out
        .get_array("log")
        .map(|entries| {
            entries.iter().filter_map(|e| e.as_str()).map(|s| json!(s)).collect()
        })
        .unwrap_or_default();
    // The newest entries are the ones worth reading.
    let shown: Vec<Value> = all.iter().rev().take(limit).rev().cloned().collect();

    Ok(untrusted(
        &format!("{} of {} server log entries", shown.len(), all.len()),
        &json!({ "logs": shown, "totalLinesAvailable": all.len() }),
    ))
}
