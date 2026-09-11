//! Reading data: find, aggregate, count, explain and export.

use bson::{Bson, Document};
use mongodb::options::{AggregateOptions, FindOptions};
use serde_json::{Map, Value, json};

use super::{
    Context, ToolError, assert_no_server_side_js, assert_writable, collect_limited,
    db_and_collection, err, required_str, to_document, to_pipeline, untrusted, write_stage,
};

/// Byte ceiling for one response, bounded by the configured maximum.
fn response_bytes(ctx: &Context, args: &Map<String, Value>) -> usize {
    let asked = args
        .get("responseBytesLimit")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(ctx.config.max_bytes_per_query);
    asked.min(ctx.config.max_bytes_per_query)
}

/// Refuse a query the server would answer with a collection scan.
///
/// A scan on a large collection is slow and hits every document, so when the
/// check is on it is better to say so than to run it.
async fn assert_uses_an_index(
    ctx: &Context,
    conn: &crate::session::Connection,
    database: &str,
    command: Document,
) -> Result<(), ToolError> {
    if !ctx.config.index_check {
        return Ok(());
    }
    let plan = conn
        .client
        .database(database)
        .run_command(bson::doc! { "explain": command, "verbosity": "queryPlanner" })
        .await?;
    let plan_json = Bson::Document(plan).into_relaxed_extjson();
    if let Some(stage) = find_collection_scan(&plan_json) {
        return err(format!(
            "this query would be answered with a {stage}, reading every document in the              collection; add an index that covers it, or set MDB_MCP_INDEX_CHECK=false to allow              unindexed queries"
        ));
    }
    Ok(())
}

/// Look for a scan stage anywhere in the winning plan.
fn find_collection_scan(plan: &Value) -> Option<&str> {
    match plan {
        Value::Object(map) => {
            if let Some(stage) = map.get("stage").and_then(Value::as_str)
                && matches!(stage, "COLLSCAN")
            {
                return Some(stage);
            }
            map.values().find_map(find_collection_scan)
        }
        Value::Array(items) => items.iter().find_map(find_collection_scan),
        _ => None,
    }
}

pub async fn find(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;

    if let Some(filter) = args.get("filter") {
        assert_no_server_side_js(&ctx.config, filter)?;
    }
    let filter = to_document(args.get("filter"), "filter")?;
    let projection = to_document(args.get("projection"), "projection")?;
    let sort = to_document(args.get("sort"), "sort")?;

    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|v| v as usize)
        .unwrap_or(10)
        .min(ctx.config.max_documents_per_query);

    let mut options = FindOptions::default();
    if !projection.is_empty() {
        options.projection = Some(projection);
    }
    if !sort.is_empty() {
        options.sort = Some(sort);
    }
    options.limit = Some(limit as i64);
    if let Some(ms) = ctx.config.max_time_ms {
        options.max_time = Some(std::time::Duration::from_millis(ms));
    }

    let mut check = bson::doc! { "find": &collection, "filter": filter.clone() };
    if let Some(sort) = options.sort.clone() {
        check.insert("sort", sort);
    }
    assert_uses_an_index(ctx, &conn, &database, check).await?;

    let cursor = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .find(filter)
        .with_options(options)
        .await?;

    let (documents, applied) = collect_limited(cursor, limit, response_bytes(ctx, args)).await?;
    Ok(untrusted(
        &format!("{} documents from {database}.{collection}", documents.len()),
        &json!({
            "documents": documents,
            "queryResultsCount": documents.len(),
            "appliedLimits": applied,
        }),
    ))
}

pub async fn aggregate(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    if let Some(p) = args.get("pipeline") {
        assert_no_server_side_js(&ctx.config, p)?;
    }
    let pipeline = to_pipeline(args.get("pipeline"))?;

    // `$out` and `$merge` write, so read-only mode has to see them.
    if let Some(stage) = pipeline.iter().find_map(write_stage) {
        assert_writable(&ctx.config, &format!("an aggregation with {stage}"))?;
    }

    let mut options = AggregateOptions::default();
    if let Some(ms) = ctx.config.max_time_ms {
        options.max_time = Some(std::time::Duration::from_millis(ms));
    }
    let cursor = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .aggregate(pipeline)
        .with_options(options)
        .await?;

    let (documents, applied) = collect_limited(
        cursor,
        ctx.config.max_documents_per_query,
        response_bytes(ctx, args),
    )
    .await?;
    Ok(untrusted(
        &format!("{} documents from aggregating {database}.{collection}", documents.len()),
        &json!({ "documents": documents, "appliedLimits": applied }),
    ))
}

/// A database-level aggregation, for stages such as `$listLocalSessions`.
pub async fn aggregate_db(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let database = required_str(args, "database")?;
    let conn = ctx.connection(args)?;
    if let Some(p) = args.get("pipeline") {
        assert_no_server_side_js(&ctx.config, p)?;
    }
    let pipeline = to_pipeline(args.get("pipeline"))?;
    if let Some(stage) = pipeline.iter().find_map(write_stage) {
        assert_writable(&ctx.config, &format!("an aggregation with {stage}"))?;
    }

    let cursor = conn.client.database(&database).aggregate(pipeline).await?;
    let (documents, applied) = collect_limited(
        cursor,
        ctx.config.max_documents_per_query,
        response_bytes(ctx, args),
    )
    .await?;
    Ok(untrusted(
        &format!("{} documents from aggregating database {database}", documents.len()),
        &json!({ "documents": documents, "appliedLimits": applied }),
    ))
}

pub async fn count(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    if let Some(q) = args.get("query") {
        assert_no_server_side_js(&ctx.config, q)?;
    }
    let filter = to_document(args.get("query"), "query")?;
    let mut check = bson::doc! { "count": &collection };
    if !filter.is_empty() {
        check.insert("query", filter.clone());
    }
    assert_uses_an_index(ctx, &conn, &database, check).await?;

    let count = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .count_documents(filter)
        .await?;
    Ok(json!({ "count": count, "database": database, "collection": collection }))
}

pub async fn explain(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    let verbosity = args
        .get("verbosity")
        .and_then(Value::as_str)
        .unwrap_or("queryPlanner")
        .to_string();

    let Some(method) = args.get("method").and_then(Value::as_array).and_then(|m| m.first()) else {
        return err("method must be a one-element array naming the operation to explain");
    };
    let name = method.get("name").and_then(Value::as_str).unwrap_or_default();
    let arguments = method.get("arguments").cloned().unwrap_or_else(|| json!({}));
    assert_no_server_side_js(&ctx.config, &arguments)?;

    // `explain` takes the command it would have run as its argument.
    let inner = match name {
        "find" => {
            let mut cmd = bson::doc! { "find": &collection };
            let filter = to_document(arguments.get("filter"), "filter")?;
            if !filter.is_empty() {
                cmd.insert("filter", filter);
            }
            let sort = to_document(arguments.get("sort"), "sort")?;
            if !sort.is_empty() {
                cmd.insert("sort", sort);
            }
            let projection = to_document(arguments.get("projection"), "projection")?;
            if !projection.is_empty() {
                cmd.insert("projection", projection);
            }
            cmd
        }
        "aggregate" => bson::doc! {
            "aggregate": &collection,
            "pipeline": to_pipeline(arguments.get("pipeline"))?,
            "cursor": {},
        },
        "count" => {
            let mut cmd = bson::doc! { "count": &collection };
            let query = to_document(arguments.get("query"), "query")?;
            if !query.is_empty() {
                cmd.insert("query", query);
            }
            cmd
        }
        other => return err(format!("explain does not support the {other:?} method")),
    };

    let out = conn
        .client
        .database(&database)
        .run_command(bson::doc! { "explain": inner, "verbosity": verbosity })
        .await?;
    Ok(Bson::Document(out).into_relaxed_extjson())
}

/// The reference writes a file and hands back a URI; without that transport
/// this returns the documents inline, capped like every other read.
pub async fn export(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;

    let target = args.get("exportTarget").cloned().unwrap_or_else(|| json!({}));
    let pipeline = match target.get("pipeline") {
        Some(p) => {
            assert_no_server_side_js(&ctx.config, p)?;
            to_pipeline(Some(p))?
        }
        None => {
            let filter = to_document(target.get("filter"), "filter")?;
            vec![bson::doc! { "$match": filter }]
        }
    };

    let cursor = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .aggregate(pipeline)
        .await?;
    let (documents, applied) =
        collect_limited(cursor, ctx.config.max_documents_per_query, ctx.config.max_bytes_per_query)
            .await?;

    Ok(untrusted(
        &format!("{} documents exported from {database}.{collection}", documents.len()),
        &json!({
            "documents": documents,
            "appliedLimits": applied,
            "note": "this server returns exports inline rather than writing a file",
        }),
    ))
}
