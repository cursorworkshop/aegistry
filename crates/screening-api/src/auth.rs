use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::{ApiError, AppState};

#[derive(Clone)]
pub struct ApiKeyAuth {
    pub tenant_id: String,
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, axum::Json<ApiError>)> {
    // Extract API key from header
    let api_key = request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let api_key = match api_key {
        Some(key) => key,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiError {
                    message: "missing_api_key".to_string(),
                    details: vec!["X-Api-Key header is required".to_string()],
                }),
            ));
        }
    };

    // Validate API key
    let tenant = match state.tenant_store.get_tenant_by_key(&api_key) {
        Some(t) => t,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                axum::Json(ApiError {
                    message: "invalid_api_key".to_string(),
                    details: vec!["API key is invalid or expired".to_string()],
                }),
            ));
        }
    };

    // Check if tenant is active
    if !tenant.is_active {
        return Err((
            StatusCode::FORBIDDEN,
            axum::Json(ApiError {
                message: "tenant_suspended".to_string(),
                details: vec!["Tenant account is suspended".to_string()],
            }),
        ));
    }

    // Add auth info to request extensions
    request.extensions_mut().insert(ApiKeyAuth {
        tenant_id: tenant.id.clone(),
    });

    Ok(next.run(request).await)
}

