use std::ops::Deref;
use std::time::Duration;

use crate::nodes::definition::NodeHandler;
use crate::nodes::function::v2::error::FunctionResult;
use crate::nodes::function::v2::function::{Function, HandlerResponse};
use crate::nodes::function::v2::module::console::Log;
use crate::nodes::function::v2::serde::JsValue;
use crate::nodes::result::NodeResult;
use crate::nodes::{NodeContext, NodeContextExt};
use ::serde::{Deserialize, Serialize};
use rquickjs::{async_with, CatchResultExt, Object};
use serde_json::json;
use zen_expression::variable::ToVariable;
use zen_types::decision::FunctionContent;

pub(crate) mod error;
pub(crate) mod function;
pub(crate) mod listener;
pub(crate) mod module;
pub(crate) mod serde;

#[derive(Debug, Clone)]
pub struct FunctionV2NodeHandler;

impl NodeHandler for FunctionV2NodeHandler {
    type NodeData = FunctionContent;
    type TraceData = FunctionV2Trace;

    async fn handle(&self, ctx: NodeContext<Self::NodeData, Self::TraceData>) -> NodeResult {
        let start = std::time::Instant::now();

        if ctx.node.omit_nodes {
            ctx.input.dot_remove("$nodes");
        }

        let function = ctx.function_runtime().await?;
        let module_name = function.suggest_module_name(ctx.id.deref(), ctx.node.source.deref());

        let max_duration = Duration::from_millis(ctx.config.function_timeout_millis);
        let interrupt_handler = Box::new(move || start.elapsed() > max_duration);

        function
            .runtime()
            .set_interrupt_handler(Some(interrupt_handler))
            .await;

        self.attach_globals(function, &ctx)
            .await
            .node_context(&ctx)?;

        function
            .register_module(&module_name, ctx.node.source.deref())
            .await
            .node_context(&ctx)?;

        let response_result = function
            .call_handler(&module_name, JsValue(ctx.input.clone()))
            .await;

        match response_result {
            Ok(response) => {
                function.runtime().set_interrupt_handler(None).await;
                ctx.trace(|t| {
                    t.log = response.logs.clone();
                });

                ctx.success(response.data)
            }
            Err(e) => {
                let log = function.extract_logs().await;
                ctx.trace(|t| {
                    t.log = log;
                    t.log.push(Log {
                        lines: vec![json!(e.to_string()).to_string()],
                        ms_since_run: start.elapsed().as_millis() as usize,
                    });
                });

                ctx.error(e)
            }
        }
    }
}

impl FunctionV2NodeHandler {
    async fn attach_globals(
        &self,
        function: &Function,
        node_ctx: &NodeContext<FunctionContent, FunctionV2Trace>,
    ) -> FunctionResult {
        async_with!(function.context() => |ctx| {
            let config = Object::new(ctx.clone()).catch(&ctx)?;

            config.prop("iteration", node_ctx.iteration).catch(&ctx)?;
            config.prop("maxDepth", node_ctx.config.max_depth).catch(&ctx)?;
            config.prop("trace", node_ctx.config.trace).catch(&ctx)?;

            ctx.globals().set("config", config).catch(&ctx)?;

            Ok(())
        })
        .await
    }
}

#[derive(Debug, Clone, Default, ToVariable)]
#[serde(rename_all = "camelCase")]
pub struct FunctionV2Trace {
    pub log: Vec<Log>,
}

#[derive(Serialize, Deserialize)]
pub struct FunctionResponse {
    performance: String,
    data: Option<HandlerResponse>,
}
