use crate::custom_node::{DynamicCustomNode, ZenCustomNodeResult};
use crate::engine::{ZenEngine, ZenEngineStruct};
use crate::loader::{DynamicDecisionLoader, ZenDecisionLoaderResult};
use std::ffi::{c_char, CString};
use std::future::Future;
use std::pin::Pin;
use zen_engine::loader::{DecisionLoader, LoaderResponse};
use zen_engine::nodes::custom::{CustomNodeAdapter, CustomNodeRequest};
use zen_engine::nodes::{NodeError, NodeResult};

pub type ZenDecisionLoaderNativeCallback =
    extern "C" fn(key: *const c_char) -> ZenDecisionLoaderResult;

pub type ZenCustomNodeNativeCallback = extern "C" fn(request: *const c_char) -> ZenCustomNodeResult;

#[derive(Debug)]
pub(crate) struct NativeDecisionLoader {
    callback: ZenDecisionLoaderNativeCallback,
}

impl NativeDecisionLoader {
    pub fn new(callback: ZenDecisionLoaderNativeCallback) -> Self {
        Self { callback }
    }
}

impl DecisionLoader for NativeDecisionLoader {
    fn load<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = LoaderResponse> + Send + 'a>> {
        Box::pin(async move {
            let c_key = CString::new(key).unwrap();
            let c_content_ptr = (&self.callback)(c_key.as_ptr());

            c_content_ptr.into_loader_response(key)
        })
    }
}

#[derive(Debug)]
pub(crate) struct NativeCustomNode {
    callback: ZenCustomNodeNativeCallback,
}

impl NativeCustomNode {
    pub fn new(callback: ZenCustomNodeNativeCallback) -> Self {
        Self { callback }
    }
}

impl CustomNodeAdapter for NativeCustomNode {
    fn handle(&self, request: CustomNodeRequest) -> Pin<Box<dyn Future<Output = NodeResult> + '_>> {
        Box::pin(async move {
            let Ok(request_value) = serde_json::to_string(&request) else {
                return Err(NodeError {
                    node_id: request.node.id.clone(),
                    trace: None,
                    source: "failed to serialize request json".into(),
                });
            };

            let c_request = unsafe { CString::from_vec_unchecked(request_value.into_bytes()) };
            let c_response_str = (&self.callback)(c_request.as_ptr());
            c_response_str.into_node_result().map_err(|err| NodeError {
                node_id: request.node.id.clone(),
                trace: None,
                source: err.into(),
            })
        })
    }
}

/// Creates a new ZenEngine instance with loader, caller is responsible for freeing the returned reference
/// by calling zen_engine_free.
#[no_mangle]
pub extern "C" fn zen_engine_new_native(
    loader_callback: ZenDecisionLoaderNativeCallback,
    custom_node_callback: ZenCustomNodeNativeCallback,
) -> *mut ZenEngineStruct {
    let loader = NativeDecisionLoader::new(loader_callback);
    let custom_node = NativeCustomNode::new(custom_node_callback);

    let engine = ZenEngine::new(
        DynamicDecisionLoader::Native(loader),
        DynamicCustomNode::Native(custom_node),
    );

    Box::into_raw(Box::new(engine)) as *mut ZenEngineStruct
}
