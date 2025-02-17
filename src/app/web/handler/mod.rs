use crate::app::dependencies::DynDependencyProvider;
use crate::app::model::ModuleSource::OCI;
use crate::app::model::{
    ExecuteTemplateRequest, ExecuteTemplateResponse, ExecuteTemplateResult, Parameter, Phase,
    PluginInvocation,
};
use crate::app::wasm::{Runner, WasmError};
use axum::extract::Extension;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_macros::debug_handler;
use tracing::{debug, error};

#[debug_handler]
pub async fn execute_template(
    Json(request): Json<ExecuteTemplateRequest>,
    Extension(deps): Extension<DynDependencyProvider>,
) -> Result<Json<ExecuteTemplateResponse>, AppError> {
    debug!("Request: {:?}", request);

    let module_source = match request.template.plugin.wasm {
        Some(config) => config.module,
        None => return Ok(ExecuteTemplateResponse { node: None }.into()),
    };

    let OCI(image) = module_source;

    let mut in_params: Vec<Parameter> = Vec::new();
    if let Some(params) = request.template.inputs.parameters {
        in_params = params;
    }

    let invocation = PluginInvocation {
        workflow_name: request.workflow.metadata.name,
        plugin_options: vec![], // TODO fill
        parameters: in_params,
    };

    let insecure_oci_registries: Vec<&str> = deps
        .get_config()
        .insecure_oci_registries
        .iter()
        .map(AsRef::as_ref)
        .collect();
    let runner = Runner::new(
        deps.get_wasm_engine(),
        deps.get_module_cache(),
        &insecure_oci_registries,
    );

    match runner.run(&image, invocation).await {
        Ok(result) => {
            let response = ExecuteTemplateResponse { node: Some(result) };
            debug!(?response, "Send Response");
            Ok(response.into())
        }
        Err(err) => {
            error!(?err, "Send Error");
            Err(err.into())
        }
    }
}

pub enum AppError {
    ModuleExecution(WasmError),
}

impl From<WasmError> for AppError {
    fn from(inner: WasmError) -> Self {
        AppError::ModuleExecution(inner)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::ModuleExecution(WasmError::EnvironmentSetup(_err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Wasm environment is not set up correctly",
            ),
            AppError::ModuleExecution(WasmError::Invocation(_err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Wasm module invocation failed",
            ),
            AppError::ModuleExecution(WasmError::OutputProcessing(_err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Wasm module output processing failed",
            ),
            AppError::ModuleExecution(WasmError::Retrieve(_err)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Wasm module could not be retrieved",
            ),
            AppError::ModuleExecution(WasmError::Precompile(_)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Wasm module could not be precompiled",
            ),
        };

        let response = Json(ExecuteTemplateResponse {
            node: Some(ExecuteTemplateResult {
                phase: Phase::Failed,
                message: error_message.to_string(),
                outputs: None,
            }),
        });

        (status, response).into_response()
    }
}
