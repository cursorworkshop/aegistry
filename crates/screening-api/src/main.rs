use aegistry_core::{
    health_status, new_request_id, HealthStatus, Hit, RiskLevel, ScreenPersonRequest,
    ScreenPersonResponse, VersionResponse, PROJECT_NAME, PROJECT_VERSION,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    middleware,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use matching_core::{score_against_stub, MatchingEngine};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use utoipa::ToSchema;
use std::sync::Arc;
use std::time::Instant;
use std::{env, net::SocketAddr};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use validator::Validate;

mod analytics;
mod audit;
mod auth;
mod batch;
mod risk;
mod tenant;
mod tenant_db;
mod webhooks;

use auth::auth_middleware;
use batch::{BatchJob, BatchRequest, BatchResponse, BatchStatus, BatchResult};
use ingest::monitoring::{
    add_monitored_subject, get_pending_notifications, mark_notified, remove_monitored_subject,
    compute_result_hash,
};
use tenant::TenantStore;
use tenant_db::{ensure_default_tenant, open_tenant_db};

const SERVICE_NAME: &str = "screening-api";

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub engine: Option<Arc<MatchingEngine>>,
    pub tenant_store: Arc<TenantStore>,
    pub batch_jobs: Arc<tokio::sync::RwLock<std::collections::HashMap<String, BatchJob>>>,
    pub monitoring_db: Arc<tokio::sync::Mutex<rusqlite::Connection>>,
    pub audit_store: Arc<audit::AuditStore>,
    pub risk_store: Arc<risk::RiskStore>,
    pub analytics_store: Arc<analytics::AnalyticsStore>,
}

#[tokio::main]
async fn main() {
    init_tracing();

    let cfg = AppConfig::from_env();
    
    // Initialize metrics
    init_metrics();
    
    // Try to load real matching engine
    let engine = load_matching_engine(&cfg);
    if engine.is_some() {
        tracing::info!("real sanctions data loaded");
    } else {
        tracing::warn!("no sanctions data found, using stub data. Run 'cargo run -p ingest' first.");
    }

    // Initialize tenant store
    let tenant_store = Arc::new(TenantStore::new(&cfg.data_dir));
    
    // Always ensure default tenant exists in memory store (used for auth)
    tenant_store.create_default_tenant();
    
    // Also try to use persistent tenant DB for future use
    match open_tenant_db(&cfg.data_dir) {
        Ok(conn) => {
            if let Err(e) = ensure_default_tenant(&conn) {
                tracing::warn!(error = %e, "failed to ensure default tenant in persistent DB");
            } else {
                tracing::info!("persistent tenant storage initialized");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open tenant DB, using in-memory store only");
        }
    }

    // Open monitoring database (same as main aegistry.db)
    let db_path = std::path::PathBuf::from(&cfg.data_dir).join("aegistry.db");
    let monitoring_db = match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            ingest::monitoring::init_monitoring_schema(&conn).ok();
            Arc::new(tokio::sync::Mutex::new(conn))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open monitoring DB");
            // Create a dummy connection for now - monitoring won't work but API will
            Arc::new(tokio::sync::Mutex::new(rusqlite::Connection::open_in_memory().unwrap()))
        }
    };

    // Initialize audit log database
    let audit_db_path = std::path::PathBuf::from(&cfg.data_dir).join("audit.db");
    let audit_db = match rusqlite::Connection::open(&audit_db_path) {
        Ok(conn) => {
            audit::AuditStore::init_schema(&conn).ok();
            Arc::new(audit::AuditStore::new(conn))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open audit DB");
            Arc::new(audit::AuditStore::new(rusqlite::Connection::open_in_memory().unwrap()))
        }
    };

    // Initialize risk config database
    let risk_db_path = std::path::PathBuf::from(&cfg.data_dir).join("risk.db");
    let risk_store = match rusqlite::Connection::open(&risk_db_path) {
        Ok(conn) => {
            risk::RiskStore::init_schema(&conn).ok();
            Arc::new(risk::RiskStore::new(conn))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open risk DB");
            Arc::new(risk::RiskStore::new(rusqlite::Connection::open_in_memory().unwrap()))
        }
    };

    // Initialize analytics database
    let analytics_db_path = std::path::PathBuf::from(&cfg.data_dir).join("analytics.db");
    let analytics_store = match rusqlite::Connection::open(&analytics_db_path) {
        Ok(conn) => {
            analytics::AnalyticsStore::init_schema(&conn).ok();
            Arc::new(analytics::AnalyticsStore::new(conn))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open analytics DB");
            Arc::new(analytics::AnalyticsStore::new(rusqlite::Connection::open_in_memory().unwrap()))
        }
    };

    let state = AppState {
        config: cfg.clone(),
        engine: engine.clone(),
        tenant_store,
        batch_jobs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        monitoring_db: monitoring_db.clone(),
        audit_store: audit_db.clone(),
        risk_store: risk_store.clone(),
        analytics_store: analytics_store.clone(),
    };

    // Start background callback task
    if engine.is_some() {
        let callback_state = state.clone();
        tokio::spawn(async move {
            monitoring_callback_loop(callback_state).await;
        });
    }

    let app = build_router(state);
    let addr: SocketAddr = cfg
        .bind_addr
        .parse()
        .expect("BIND_ADDR must be a valid socket address, e.g. 0.0.0.0:3000");

    let listener = TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {cfg:?}: {e}. Hint: set BIND_ADDR=127.0.0.1:3101"));

    tracing::info!(
        %addr,
        service = SERVICE_NAME,
        project = PROJECT_NAME,
        env = %cfg.run_env,
        "listening"
    );

    axum::serve(listener, app)
        .await
        .expect("server error while serving requests");
}

fn load_matching_engine(cfg: &AppConfig) -> Option<Arc<MatchingEngine>> {
    let index_path = PathBuf::from(&cfg.data_dir).join("index");
    let db_path = PathBuf::from(&cfg.data_dir).join("aegistry.db");

    if !index_path.exists() || !db_path.exists() {
        return None;
    }

    match MatchingEngine::open(&index_path, &db_path) {
        Ok(engine) => Some(Arc::new(engine)),
        Err(e) => {
            tracing::warn!(error = %e, "failed to load matching engine");
            None
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    // Public routes (no auth)
    let public_routes = Router::new()
        .route("/", get(index_page))
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler));

    // Protected routes (require API key)
    let protected_routes = Router::new()
        .route("/v1/version", get(version))
        .route("/v1/persons/screen", post(screen_person))
        .route("/v1/entities/screen", post(screen_entity))
        .route("/v1/batch", post(create_batch))
        .route("/v1/batch/:job_id", get(get_batch_status))
        .route("/v1/batch/:job_id/results", get(get_batch_results))
        .route("/v1/monitoring", post(add_monitoring))
        .route("/v1/monitoring", get(list_monitoring))
        .route("/v1/monitoring/:reference_id", axum::routing::delete(remove_monitoring))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .with_state(state)
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hyper=warn,tower_http=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn init_metrics() {
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    builder
        .install_recorder()
        .expect("failed to install Prometheus recorder");
}

async fn index_page() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Health check", body = HealthStatus)
    )
)]
async fn health() -> Json<HealthStatus<'static>> {
    Json(health_status(SERVICE_NAME))
}

#[utoipa::path(
    get,
    path = "/v1/version",
    tag = "health",
    responses(
        (status = 200, description = "Version info", body = VersionResponse)
    )
)]
async fn version() -> Json<VersionResponse<'static>> {
    Json(VersionResponse {
        service: SERVICE_NAME,
        project: PROJECT_NAME,
        version: PROJECT_VERSION,
    })
}

async fn metrics_handler() -> impl IntoResponse {
    // Metrics are collected but we return a simple status for now
    // Full Prometheus export requires storing the handle globally
    (StatusCode::OK, "# Metrics endpoint\n# Use Prometheus scraping for full metrics\n")
}

async fn screen_person(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ScreenPersonRequest>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let start = Instant::now();
    counter!("screening_requests_total", "type" => "person").increment(1);

    if let Err(e) = req.validate() {
        counter!("screening_errors_total", "type" => "validation").increment(1);
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::from_validation(e)),
        ));
    }

    let hits = perform_screening(&state, &req.full_name(), req.country.as_deref(), req.dob_year());

    let response = ScreenPersonResponse {
        request_id: new_request_id(),
        reference_id: req.reference_id,
        hits,
        checked_at: Utc::now().to_rfc3339(),
    };

    histogram!("screening_latency_seconds", "type" => "person").record(start.elapsed().as_secs_f64());

    format_response(&headers, &response)
}

#[utoipa::path(
    post,
    path = "/v1/entities/screen",
    tag = "screening",
    request_body = ScreenEntityRequest,
    responses(
        (status = 200, description = "Screening results", body = ScreenEntityResponse),
        (status = 422, description = "Validation error", body = ApiError)
    )
)]
async fn screen_entity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ScreenEntityRequest>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let start = Instant::now();
    counter!("screening_requests_total", "type" => "entity").increment(1);

    if let Err(e) = req.validate() {
        counter!("screening_errors_total", "type" => "validation").increment(1);
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::from_validation(e)),
        ));
    }

    let hits = perform_screening(&state, &req.name, req.country.as_deref(), None);

    let response = ScreenEntityResponse {
        request_id: new_request_id(),
        reference_id: req.reference_id,
        hits,
        checked_at: Utc::now().to_rfc3339(),
    };

    histogram!("screening_latency_seconds", "type" => "entity").record(start.elapsed().as_secs_f64());

    format_response(&headers, &response)
}

fn perform_screening(
    state: &AppState,
    name: &str,
    country: Option<&str>,
    dob_year: Option<i32>,
) -> Vec<Hit> {
    if let Some(ref engine) = state.engine {
        let matches = engine.search_and_score(name, country, dob_year, 10);

        matches
            .into_iter()
            .map(|m| {
                let explanation = m.components.explain(&m.primary_name, m.country.as_deref());
                Hit {
                    subject_id: m.subject_id,
                    matched_name: m.primary_name,
                    source: m.source,
                    kind: m.kind,
                    score: m.score,
                    risk_level: if m.score >= 0.95 {
                        RiskLevel::Hit
                    } else if m.score >= 0.90 {
                        RiskLevel::Review
                    } else {
                        RiskLevel::None
                    },
                    components: m.components,
                    explanation,
                }
            })
            .collect()
    } else {
        let matches = score_against_stub(name, country, dob_year, 5);

        matches
            .into_iter()
            .map(|m| {
                let explanation = m.components.explain(m.subject.name, m.subject.country);
                Hit {
                    subject_id: m.subject.subject_id.to_string(),
                    matched_name: m.subject.name.to_string(),
                    source: m.subject.source,
                    kind: m.subject.kind,
                    score: m.score,
                    risk_level: if m.score >= 0.95 {
                        RiskLevel::Hit
                    } else if m.score >= 0.90 {
                        RiskLevel::Review
                    } else {
                        RiskLevel::None
                    },
                    components: m.components,
                    explanation,
                }
            })
            .collect()
    }
}

async fn create_batch(
    State(state): State<AppState>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, (StatusCode, Json<ApiError>)> {
    counter!("batch_requests_total").increment(1);

    let total_records = req.records.len();
    let job_id = new_request_id();
    let job = BatchJob::new(job_id.clone(), total_records);

    // Store job
    {
        let mut jobs = state.batch_jobs.write().await;
        jobs.insert(job_id.clone(), job.clone());
    }

    // Process in background
    let state_clone = state.clone();
    let records = req.records;
    let jid = job_id.clone();
    tokio::spawn(async move {
        batch::process_batch(state_clone, jid, records).await;
    });

    Ok(Json(BatchResponse {
        job_id,
        status: BatchStatus::Processing,
        total_records,
        processed_records: 0,
        created_at: Utc::now().to_rfc3339(),
    }))
}

async fn get_batch_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<BatchResponse>, (StatusCode, Json<ApiError>)> {
    let jobs = state.batch_jobs.read().await;
    
    match jobs.get(&job_id) {
        Some(job) => Ok(Json(BatchResponse {
            job_id: job.id.clone(),
            status: job.status.clone(),
            total_records: job.total_records,
            processed_records: job.processed_records,
            created_at: job.created_at.clone(),
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                message: "job_not_found".to_string(),
                details: vec![format!("Job {} not found", job_id)],
            }),
        )),
    }
}

#[utoipa::path(
    get,
    path = "/v1/batch/{job_id}/results",
    tag = "batch",
    params(
        ("job_id" = String, Path, description = "Batch job ID")
    ),
    responses(
        (status = 200, description = "Batch results", body = Vec<BatchResult>),
        (status = 400, description = "Job not complete", body = ApiError),
        (status = 404, description = "Job not found", body = ApiError)
    )
)]
async fn get_batch_results(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let jobs = state.batch_jobs.read().await;
    
    match jobs.get(&job_id) {
        Some(job) => {
            if job.status != BatchStatus::Completed {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        message: "job_not_complete".to_string(),
                        details: vec!["Job is still processing".to_string()],
                    }),
                ));
            }
            format_response(&headers, &job.results)
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                message: "job_not_found".to_string(),
                details: vec![format!("Job {} not found", job_id)],
            }),
        )),
    }
}

// Monitoring endpoints
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct AddMonitoringRequest {
    pub reference_id: String,
    #[validate(length(min = 1))]
    pub name: String,
    pub country: Option<String>,
    pub dob_year: Option<i32>,
    pub callback_url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MonitoringEntry {
    pub reference_id: String,
    pub name: String,
    pub country: Option<String>,
    pub dob_year: Option<i32>,
    pub last_screened_at: String,
    pub callback_url: Option<String>,
}

async fn add_monitoring(
    State(state): State<AppState>,
    Json(req): Json<AddMonitoringRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    if let Err(e) = req.validate() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError::from_validation(e)),
        ));
    }

    // Extract tenant_id from auth (for now use "default")
    let tenant_id = "default"; // TODO: get from auth middleware

    // Store in SQLite
    let db = state.monitoring_db.lock().await;
    match add_monitored_subject(
        &*db,
        tenant_id,
        &req.reference_id,
        &req.name,
        req.country.as_deref(),
        req.dob_year,
        req.callback_url.as_deref(),
    ) {
        Ok(_) => {
            tracing::info!(
                reference_id = %req.reference_id,
                name = %req.name,
                "added subject to monitoring"
            );

            // Perform initial screening
            let hits = perform_screening(&state, &req.name, req.country.as_deref(), req.dob_year);
            let hit_data: Vec<(String, f32)> = hits.iter().map(|h| (h.subject_id.clone(), h.score)).collect();
            let result_hash = compute_result_hash(&hit_data);

            // Record initial result
            let db = state.monitoring_db.lock().await;
            if let Ok(subjects) = ingest::monitoring::get_monitored_subjects(&*db, tenant_id) {
                if let Some(subject) = subjects.iter().find(|s| s.reference_id == req.reference_id) {
                    let _ = ingest::monitoring::record_monitoring_result(
                        &*db,
                        subject.id,
                        &result_hash,
                        hits.len(),
                        hits.first().map(|h| h.score).unwrap_or(0.0),
                        false, // No changes on initial screening
                    );
                }
            }
            drop(db);

            Ok(Json(serde_json::json!({
                "status": "added",
                "reference_id": req.reference_id,
                "message": "Subject will be re-screened when sanctions lists update"
            })))
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to add monitoring");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    message: "monitoring_error".to_string(),
                    details: vec![format!("Failed to add monitoring: {}", e)],
                }),
            ))
        }
    }
}

#[utoipa::path(
    get,
    path = "/v1/monitoring",
    tag = "monitoring",
    responses(
        (status = 200, description = "List of monitored subjects", body = Vec<MonitoringEntry>)
    )
)]
async fn list_monitoring(
    State(state): State<AppState>,
) -> Result<Json<Vec<MonitoringEntry>>, (StatusCode, Json<ApiError>)> {
    let tenant_id = "default"; // TODO: get from auth
    
    let db = state.monitoring_db.lock().await;
    match ingest::monitoring::get_monitored_subjects(&*db, tenant_id) {
        Ok(subjects) => {
            let entries: Vec<MonitoringEntry> = subjects
                .into_iter()
                .map(|s| MonitoringEntry {
                    reference_id: s.reference_id,
                    name: s.name,
                    country: s.country,
                    dob_year: s.dob_year,
                    last_screened_at: s.last_screened_at,
                    callback_url: s.callback_url,
                })
                .collect();
            drop(db);
            Ok(Json(entries))
        }
        Err(e) => {
            drop(db);
            tracing::error!(error = %e, "failed to list monitoring");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    message: "monitoring_error".to_string(),
                    details: vec![format!("Failed to list monitoring: {}", e)],
                }),
            ))
        }
    }
}

#[utoipa::path(
    delete,
    path = "/v1/monitoring/{reference_id}",
    tag = "monitoring",
    params(
        ("reference_id" = String, Path, description = "Reference ID")
    ),
    responses(
        (status = 200, description = "Monitoring removed"),
        (status = 404, description = "Not found", body = ApiError)
    )
)]
async fn remove_monitoring(
    State(state): State<AppState>,
    Path(reference_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let tenant_id = "default"; // TODO: get from auth
    
    let db = state.monitoring_db.lock().await;
    match remove_monitored_subject(&*db, tenant_id, &reference_id) {
        Ok(true) => {
            drop(db);
            tracing::info!(reference_id = %reference_id, "removed subject from monitoring");
            Ok(Json(serde_json::json!({
                "status": "removed",
                "reference_id": reference_id
            })))
        }
        Ok(false) => {
            drop(db);
            Err((
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    message: "not_found".to_string(),
                    details: vec![format!("Monitoring entry {} not found", reference_id)],
                }),
            ))
        }
        Err(e) => {
            drop(db);
            tracing::error!(error = %e, "failed to remove monitoring");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    message: "monitoring_error".to_string(),
                    details: vec![format!("Failed to remove monitoring: {}", e)],
                }),
            ))
        }
    }
}

/// Background task that processes monitoring callbacks
async fn monitoring_callback_loop(state: AppState) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    
    loop {
        interval.tick().await;
        
        // Get pending notifications
        let notifications = {
            let db = state.monitoring_db.lock().await;
            match get_pending_notifications(&*db) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to get pending notifications");
                    continue;
                }
            }
        };

        for (subject, result, result_id) in notifications {
            if let Some(callback_url) = &subject.callback_url {
                // Perform re-screening to get current hits
                let hits = perform_screening(&state, &subject.name, subject.country.as_deref(), subject.dob_year);
                
                // Get previous hits from last_result_hash (simplified - in production would store full results)
                let previous_hits: Vec<Hit> = Vec::new(); // TODO: store previous hits
                
                let callback_payload = serde_json::json!({
                    "reference_id": result.reference_id,
                    "name": subject.name,
                    "has_changes": result.has_changes,
                    "new_hits": hits,
                    "previous_hits": previous_hits,
                    "screened_at": chrono::Utc::now().to_rfc3339(),
                    "hit_count": result.hit_count,
                    "highest_score": result.highest_score,
                });

                // Send callback with retries
                let mut success = false;
                for attempt in 1..=3 {
                    let client = match reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(5))
                        .build()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::warn!(error = %e, attempt, "failed to build HTTP client");
                            continue;
                        }
                    };
                    
                    match client
                        .post(callback_url)
                        .header("Content-Type", "application/json")
                        .body(serde_json::to_string(&callback_payload).unwrap_or_default())
                        .send()
                        .await
                    {
                            Ok(resp) if resp.status().is_success() => {
                                tracing::info!(
                                    callback_url = %callback_url,
                                    reference_id = %result.reference_id,
                                    "callback sent successfully"
                                );
                                success = true;
                                break;
                            }
                            Ok(resp) => {
                                tracing::warn!(
                                    callback_url = %callback_url,
                                    status = %resp.status(),
                                    attempt,
                                    "callback failed"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    callback_url = %callback_url,
                                    error = %e,
                                    attempt,
                                    "callback error"
                                );
                            }
                        }
                    
                    // Exponential backoff
                    if attempt < 3 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(2_u64.pow(attempt))).await;
                    }
                }

                if success {
                    // Mark as notified
                    let db = state.monitoring_db.lock().await;
                    if let Err(e) = mark_notified(&*db, result_id) {
                        tracing::warn!(error = %e, result_id, "failed to mark notification as sent");
                    }
                }
            }
        }
    }
}

fn format_response<T: Serialize>(headers: &HeaderMap, response: &T) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    if accept.contains("yaml") || accept.contains("x-yaml") {
        let yaml = serde_yaml::to_string(response).unwrap_or_default();
        Ok(([(header::CONTENT_TYPE, "application/x-yaml")], yaml).into_response())
    } else {
        let json = serde_json::to_string_pretty(response).unwrap_or_default();
        Ok(([(header::CONTENT_TYPE, "application/json")], json).into_response())
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct AppConfig {
    pub bind_addr: String,
    pub run_env: String,
    pub data_dir: String,
}

impl AppConfig {
    fn from_env() -> Self {
        Self {
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3100".to_string()),
            run_env: env::var("RUN_ENV").unwrap_or_else(|_| "local".to_string()),
            data_dir: env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub message: String,
    pub details: Vec<String>,
}

impl ApiError {
    fn from_validation(err: validator::ValidationErrors) -> Self {
        let details = err
            .field_errors()
            .iter()
            .flat_map(|(field, errs)| errs.iter().map(move |e| format!("{}: {}", field, e.code)))
            .collect::<Vec<_>>();
        ApiError {
            message: "invalid_request".to_string(),
            details,
        }
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ScreenEntityRequest {
    pub reference_id: Option<String>,
    #[validate(length(min = 1))]
    pub name: String,
    #[validate(length(min = 2, max = 2))]
    pub country: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScreenEntityResponse {
    pub request_id: String,
    pub reference_id: Option<String>,
    pub hits: Vec<Hit>,
    pub checked_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        AppState {
            config: AppConfig::from_env(),
            engine: None,
            tenant_store: Arc::new(TenantStore::new("data")),
            batch_jobs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            monitoring_db: Arc::new(tokio::sync::Mutex::new(db)),
        }
    }

    #[tokio::test]
    async fn health_ok() {
        let app = build_router(test_state());
        let res = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn screen_requires_auth() {
        let app = build_router(test_state());
        let body = serde_json::json!({
            "first_name": "Maria",
            "last_name": "Garcia",
        });
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/persons/screen")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn screen_with_auth() {
        let state = test_state();
        state.tenant_store.create_default_tenant();
        let app = build_router(state);
        let body = serde_json::json!({
            "first_name": "Maria",
            "last_name": "Garcia",
        });
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/persons/screen")
                    .header("content-type", "application/json")
                    .header("x-api-key", "test-api-key")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}

// OpenAPI documentation is disabled temporarily due to version conflicts
// TODO: Re-enable when utoipa versions are aligned
// pub struct ApiDoc;
