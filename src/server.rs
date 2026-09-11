//! The MCP surface.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Map, Value};

use crate::registry::{Registry, ToolMeta};
use crate::tools::{Context, dispatch};

pub struct MongoMcp {
    registry: Arc<Registry>,
    ctx: Arc<Context>,
}

impl MongoMcp {
    pub fn new(registry: Arc<Registry>, ctx: Arc<Context>) -> Self {
        Self { registry, ctx }
    }

    fn to_mcp_tool(meta: &ToolMeta) -> Tool {
        Tool::new(
            meta.name.clone(),
            meta.description.clone(),
            Arc::new(meta.input_schema.clone()),
        )
        .annotate(
            ToolAnnotations::default()
                .read_only(meta.is_read_only())
                .destructive(meta.is_destructive())
                .idempotent(meta.operation_type == "metadata")
                .open_world(true),
        )
    }
}

impl ServerHandler for MongoMcp {
    fn get_info(&self) -> InitializeResult {
        let mut instructions = format!(
            "MongoDB access. {} tools are active. Call connect with a connection string first \
             unless one was configured at startup; when several connections are open, pass \
             connectionId to say which one a tool should use.",
            self.registry.visible().len()
        );
        if self.registry.read_only() {
            instructions.push_str(" The server runs in read-only mode, so nothing can be written.");
        }
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("mongodb-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(instructions)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self.registry.visible().iter().map(|t| Self::to_mcp_tool(t)).collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.registry.get(name).map(Self::to_mcp_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        let Some(meta) = self.registry.get(&name) else {
            return Ok(
                CallToolResult::error(vec![ContentBlock::text(format!("unknown tool {name:?}"))])
                    .into(),
            );
        };
        if !self.registry.is_allowed(meta) {
            let reason = if self.registry.read_only() && !meta.is_read_only() {
                format!("{name} writes data and the server runs in read-only mode")
            } else {
                format!("{name} is disabled by configuration")
            };
            return Ok(CallToolResult::error(vec![ContentBlock::text(reason)]).into());
        }

        let args: Map<String, Value> = request.arguments.unwrap_or_default();
        match dispatch(&self.ctx, &name, &args).await {
            Ok(value) => {
                let text = serde_json::to_string(&value)
                    .unwrap_or_else(|e| format!("could not serialise the result: {e}"));
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
            }
            // A failed query is a result the model should read and react to,
            // not a protocol error.
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())]).into()),
        }
    }
}
