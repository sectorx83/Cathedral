//! Local authenticated API used by Cathedral Studio.
//!
//! The service binds to loopback only, requires a per-process bearer token,
//! and exposes the same experiment registry, runtime, and snapshot engine as the CLI.

use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        Method, Request, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use cathedral_core::{
    create_experiment, delete_experiment, inspect_experiment, list_experiments, manifest::Manifest,
    platform::PlatformInfo, registry::Experiment, runner, snapshot, workspace::Workspace,
    CathedralError,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone)]
struct ApiState {
    data_dir: PathBuf,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<CathedralError> for ApiError {
    fn from(error: CathedralError) -> Self {
        let status = match &error {
            CathedralError::InvalidName(_) | CathedralError::Manifest(_) => StatusCode::BAD_REQUEST,
            CathedralError::NotFound(_) => StatusCode::NOT_FOUND,
            CathedralError::AlreadyExists(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct HealthView {
    status: &'static str,
    api_version: u32,
    host_os: &'static str,
    host_arch: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ExperimentView {
    id: String,
    name: String,
    description: String,
    created_at: i64,
    manifest_version: u32,
    state: &'static str,
}

impl From<Experiment> for ExperimentView {
    fn from(item: Experiment) -> Self {
        Self {
            id: item.id,
            name: item.name,
            description: item.description,
            created_at: item.created_at,
            manifest_version: item.manifest_version,
            state: item.state.as_str(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateExperimentRequest {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunRequest {
    command: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotView {
    id: String,
    name: String,
    files: usize,
    directories: usize,
    bytes: u64,
    created_at: i64,
}

impl From<snapshot::Summary> for SnapshotView {
    fn from(item: snapshot::Summary) -> Self {
        Self {
            id: item.id,
            name: item.name,
            files: item.files,
            directories: item.directories,
            bytes: item.bytes,
            created_at: item.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct MetricsView {
    pid: u32,
    state: String,
    parent_pid: u32,
    virtual_memory_kb: u64,
    resident_memory_kb: u64,
    user_cpu_ticks: u64,
    system_cpu_ticks: u64,
    read_bytes: u64,
    write_bytes: u64,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    cgroup_memory_current_bytes: Option<u64>,
    cgroup_cpu_usage_usec: Option<u64>,
    cgroup_process_count: Option<u64>,
}

impl From<runner::ProcessMetrics> for MetricsView {
    fn from(value: runner::ProcessMetrics) -> Self {
        Self {
            pid: value.pid,
            state: value.state,
            parent_pid: value.parent_pid,
            virtual_memory_kb: value.virtual_memory_kb,
            resident_memory_kb: value.resident_memory_kb,
            user_cpu_ticks: value.user_cpu_ticks,
            system_cpu_ticks: value.system_cpu_ticks,
            read_bytes: value.read_bytes,
            write_bytes: value.write_bytes,
            network_rx_bytes: value.network_rx_bytes,
            network_tx_bytes: value.network_tx_bytes,
            cgroup_memory_current_bytes: value.cgroup_memory_current_bytes,
            cgroup_cpu_usage_usec: value.cgroup_cpu_usage_usec,
            cgroup_process_count: value.cgroup_process_count,
        }
    }
}

#[derive(Debug, Serialize)]
struct RunView {
    id: String,
    pid: u32,
    started_at: u64,
    command: Vec<String>,
    state: &'static str,
    timeout_seconds: Option<u64>,
    exit_code: Option<i32>,
    termination_signal: Option<String>,
    timed_out: bool,
    metrics: Option<MetricsView>,
}

fn run_view(record: runner::RunRecord) -> Result<RunView, ApiError> {
    let state = runner::process_state(&record).label();
    let metrics = runner::metrics(&record)?.map(MetricsView::from);
    Ok(RunView {
        id: record.id.clone(),
        pid: record.pid,
        started_at: record.started_at,
        command: record.command.clone(),
        state,
        timeout_seconds: record.timeout_seconds,
        exit_code: runner::exit_status(&record),
        termination_signal: runner::termination_signal(&record),
        timed_out: runner::timed_out(&record),
        metrics,
    })
}

#[derive(Debug, Serialize)]
struct LogsView {
    run_id: String,
    content: String,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct StopView {
    run_id: String,
    stopped: bool,
}

fn experiment_workspace(data_dir: &Path, name: &str) -> Result<(Experiment, PathBuf), ApiError> {
    let experiment = inspect_experiment(data_dir, name)?;
    let workspace = Workspace::new(data_dir)?.validate(Path::new(&experiment.workspace_path))?;
    Ok((experiment, workspace))
}

fn run_root(data_dir: &Path, experiment: &Experiment) -> PathBuf {
    data_dir.join("runs").join(&experiment.id)
}

async fn require_bearer_token(
    State(token): State<String>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if supplied != Some(token.as_str()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "a valid bearer token is required".to_owned(),
            }),
        )
            .into_response();
    }

    next.run(request).await
}

/// Build the API router. The shipped binary binds it to 127.0.0.1.
pub fn router(data_dir: PathBuf, token: String) -> Router {
    let state = ApiState { data_dir };
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            "tauri://localhost".parse().expect("static origin"),
            "http://tauri.localhost".parse().expect("static origin"),
            "https://tauri.localhost".parse().expect("static origin"),
            "http://localhost:5173".parse().expect("static origin"),
            "http://127.0.0.1:5173".parse().expect("static origin"),
        ]))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/experiments", get(list).post(create))
        .route("/api/v1/experiments/:name", get(inspect).delete(remove))
        .route(
            "/api/v1/experiments/:name/snapshots",
            get(list_snapshots).post(create_snapshot),
        )
        .route(
            "/api/v1/experiments/:name/snapshots/:snapshot_id/restore",
            axum::routing::post(restore_snapshot),
        )
        .route(
            "/api/v1/experiments/:name/runs",
            get(list_runs).post(start_run),
        )
        .route("/api/v1/experiments/:name/runs/:run_id/logs", get(get_logs))
        .route(
            "/api/v1/experiments/:name/runs/:run_id/stop",
            axum::routing::post(stop_run),
        )
        .with_state(state)
        .layer(middleware::from_fn_with_state(token, require_bearer_token))
        .layer(cors)
}

async fn health(State(_state): State<ApiState>) -> Json<HealthView> {
    let host = PlatformInfo::detect();
    Json(HealthView {
        status: "ok",
        api_version: 1,
        host_os: host.operating_system,
        host_arch: host.architecture,
    })
}

async fn list(State(state): State<ApiState>) -> Result<Json<Vec<ExperimentView>>, ApiError> {
    let items = list_experiments(&state.data_dir)?;
    Ok(Json(items.into_iter().map(ExperimentView::from).collect()))
}

async fn create(
    State(state): State<ApiState>,
    Json(request): Json<CreateExperimentRequest>,
) -> Result<(StatusCode, Json<ExperimentView>), ApiError> {
    let mut manifest = Manifest::minimal(request.name.clone());
    manifest.experiment.description = request.description;
    let item = create_experiment(&state.data_dir, &request.name, manifest)?;
    Ok((StatusCode::CREATED, Json(item.into())))
}

async fn inspect(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<ExperimentView>, ApiError> {
    Ok(Json(inspect_experiment(&state.data_dir, &name)?.into()))
}

async fn remove(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    delete_experiment(&state.data_dir, &name)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_snapshots(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Vec<SnapshotView>>, ApiError> {
    let items = snapshot::list(&state.data_dir, &name)?;
    Ok(Json(items.into_iter().map(SnapshotView::from).collect()))
}

async fn create_snapshot(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<(StatusCode, Json<SnapshotView>), ApiError> {
    let item = snapshot::create(&state.data_dir, &name)?;
    Ok((StatusCode::CREATED, Json(item.into())))
}

async fn restore_snapshot(
    State(state): State<ApiState>,
    AxumPath((name, snapshot_id)): AxumPath<(String, String)>,
) -> Result<StatusCode, ApiError> {
    snapshot::restore(&state.data_dir, &name, &snapshot_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_runs(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<Vec<RunView>>, ApiError> {
    let (experiment, _) = experiment_workspace(&state.data_dir, &name)?;
    let records = runner::list_runs(&run_root(&state.data_dir, &experiment))?;
    let mut views = Vec::with_capacity(records.len());
    for record in records {
        views.push(run_view(record)?);
    }
    Ok(Json(views))
}

async fn start_run(
    State(state): State<ApiState>,
    AxumPath(name): AxumPath<String>,
    Json(request): Json<RunRequest>,
) -> Result<(StatusCode, Json<RunView>), ApiError> {
    let (experiment, workspace) = experiment_workspace(&state.data_dir, &name)?;
    let record = runner::launch(
        &workspace,
        &run_root(&state.data_dir, &experiment),
        &request.command,
    )?;
    Ok((StatusCode::CREATED, Json(run_view(record)?)))
}

async fn get_logs(
    State(state): State<ApiState>,
    AxumPath((name, run_id)): AxumPath<(String, String)>,
) -> Result<Json<LogsView>, ApiError> {
    let (experiment, _) = experiment_workspace(&state.data_dir, &name)?;
    let content = runner::logs(&run_root(&state.data_dir, &experiment), &run_id)?;
    const MAX_LOG_BYTES: usize = 1_048_576;
    let truncated = content.len() > MAX_LOG_BYTES;
    let content = if truncated {
        let mut start = content.len() - MAX_LOG_BYTES;
        while !content.is_char_boundary(start) {
            start += 1;
        }
        content[start..].to_owned()
    } else {
        content
    };
    Ok(Json(LogsView {
        run_id,
        content,
        truncated,
    }))
}

async fn stop_run(
    State(state): State<ApiState>,
    AxumPath((name, run_id)): AxumPath<(String, String)>,
) -> Result<Json<StopView>, ApiError> {
    let (experiment, _) = experiment_workspace(&state.data_dir, &name)?;
    runner::stop(&run_root(&state.data_dir, &experiment), &run_id)?;
    Ok(Json(StopView {
        run_id,
        stopped: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use cathedral_core::initialize;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn test_router(path: &Path) -> Router {
        initialize(path).unwrap();
        router(path.to_path_buf(), "studio-test-token".to_owned())
    }

    #[tokio::test]
    async fn api_rejects_requests_without_a_bearer_token() {
        let temp = tempdir().unwrap();
        let response = test_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_accepts_a_valid_token_and_reports_health() {
        let temp = tempdir().unwrap();
        let response = test_router(temp.path())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .header(AUTHORIZATION, "Bearer studio-test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn api_creates_an_experiment() {
        let temp = tempdir().unwrap();
        let response = test_router(temp.path())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/experiments")
                    .header(AUTHORIZATION, "Bearer studio-test-token")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"studio-lab","description":"Created by Studio"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
}
