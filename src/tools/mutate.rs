//! Everything that changes data or structure.

use bson::{Bson, Document};
use mongodb::IndexModel;
use mongodb::options::IndexOptions;
use serde_json::{Map, Value, json};

use super::{
    Context, ToolError, assert_no_server_side_js, assert_writable, db_and_collection, err,
    required_str, to_document,
};

pub async fn insert_many(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "insert-many")?;
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;

    let Some(Value::Array(items)) = args.get("documents") else {
        return err("documents must be an array of objects");
    };
    if items.is_empty() {
        return err("documents must contain at least one object");
    }
    let mut docs = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        docs.push(to_document(Some(item), &format!("documents[{i}]"))?);
    }

    let result = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .insert_many(docs)
        .await?;
    let ids: Vec<Value> = result
        .inserted_ids
        .into_values()
        .map(Bson::into_relaxed_extjson)
        .collect();
    Ok(json!({
        "insertedCount": ids.len(),
        "insertedIds": ids,
        "database": database,
        "collection": collection,
    }))
}

pub async fn update_many(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "update-many")?;
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;

    for key in ["filter", "update"] {
        if let Some(v) = args.get(key) {
            assert_no_server_side_js(&ctx.config, v)?;
        }
    }
    let filter = to_document(args.get("filter"), "filter")?;
    let update = to_document(args.get("update"), "update")?;
    if update.is_empty() {
        return err("update is required and must contain at least one update operator");
    }
    // A document without operators would replace rather than update.
    if !update.keys().all(|k| k.starts_with('$')) {
        return err("update must use operators such as $set; a plain document would replace the whole record");
    }
    let upsert = args.get("upsert").and_then(Value::as_bool).unwrap_or(false);

    let result = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .update_many(filter, update)
        .upsert(upsert)
        .await?;
    Ok(json!({
        "matchedCount": result.matched_count,
        "modifiedCount": result.modified_count,
        "upsertedId": result.upserted_id.map(Bson::into_relaxed_extjson),
        "database": database,
        "collection": collection,
    }))
}

pub async fn delete_many(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "delete-many")?;
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;

    if let Some(f) = args.get("filter") {
        assert_no_server_side_js(&ctx.config, f)?;
    }
    // An absent filter would empty the collection, which is never what a
    // caller means to ask for implicitly.
    let Some(filter_value) = args.get("filter").filter(|v| !v.is_null()) else {
        return err(
            "filter is required; pass {} explicitly to delete every document in the collection",
        );
    };
    let filter = to_document(Some(filter_value), "filter")?;

    let result = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .delete_many(filter)
        .await?;
    Ok(json!({
        "deletedCount": result.deleted_count,
        "database": database,
        "collection": collection,
    }))
}

pub async fn create_collection(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "create-collection")?;
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    conn.client.database(&database).create_collection(&collection).await?;
    Ok(json!({ "database": database, "collection": collection, "message": "collection created" }))
}

pub async fn create_index(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "create-index")?;
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;

    let keys = to_document(args.get("definition"), "definition")?;
    if keys.is_empty() {
        return err("definition must name at least one field, for example {\"createdAt\": -1}");
    }
    let mut model = IndexModel::builder().keys(keys).build();
    if let Some(name) = args.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) {
        model.options = Some(IndexOptions::builder().name(name.to_string()).build());
    }

    let result = conn
        .client
        .database(&database)
        .collection::<Document>(&collection)
        .create_index(model)
        .await?;
    Ok(json!({
        "indexName": result.index_name,
        "database": database,
        "collection": collection,
    }))
}

pub async fn rename_collection(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "rename-collection")?;
    let (database, collection) = db_and_collection(args)?;
    let new_name = required_str(args, "newName")?;
    let drop_target = args.get("dropTarget").and_then(Value::as_bool).unwrap_or(false);
    let conn = ctx.connection(args)?;

    // renameCollection is an admin command taking fully qualified names.
    conn.client
        .database("admin")
        .run_command(bson::doc! {
            "renameCollection": format!("{database}.{collection}"),
            "to": format!("{database}.{new_name}"),
            "dropTarget": drop_target,
        })
        .await?;
    Ok(json!({
        "database": database,
        "from": collection,
        "to": new_name,
        "message": "collection renamed",
    }))
}

pub async fn drop_collection(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "drop-collection")?;
    let (database, collection) = db_and_collection(args)?;
    let conn = ctx.connection(args)?;
    conn.client
        .database(&database)
        .collection::<Document>(&collection)
        .drop()
        .await?;
    Ok(json!({ "database": database, "collection": collection, "message": "collection dropped" }))
}

pub async fn drop_database(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "drop-database")?;
    let database = required_str(args, "database")?;
    let conn = ctx.connection(args)?;
    conn.client.database(&database).drop().await?;
    Ok(json!({ "database": database, "message": "database dropped" }))
}

pub async fn drop_index(ctx: &Context, args: &Map<String, Value>) -> Result<Value, ToolError> {
    assert_writable(&ctx.config, "drop-index")?;
    let (database, collection) = db_and_collection(args)?;
    let index_name = required_str(args, "indexName")?;
    let conn = ctx.connection(args)?;
    conn.client
        .database(&database)
        .collection::<Document>(&collection)
        .drop_index(&index_name)
        .await?;
    Ok(json!({
        "database": database,
        "collection": collection,
        "indexName": index_name,
        "message": "index dropped",
    }))
}
