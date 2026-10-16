use crate::{
    config::{Config, Principal},
    model::*,
    store::Store,
};
use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct App {
    pub store: Arc<Mutex<Store>>,
    pub identities: Arc<Vec<(String, Principal)>>,
}
impl App {
    pub fn new(store: Store, config: Config) -> Result<Self> {
        let identities = config
            .principals
            .into_iter()
            .map(|p| {
                let token = std::env::var(&p.token_env)?;
                Ok((digest(token.as_bytes()), p))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            identities: Arc::new(identities),
        })
    }
    fn with<T>(&self, f: impl FnOnce(&mut Store) -> Result<T>) -> ApiResult<T> {
        let mut store = self.store.lock().map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store unavailable".into(),
            )
        })?;
        f(&mut store).map_err(|e| {
            if e.downcast_ref::<rusqlite::Error>().is_some() {
                eprintln!("soapbox database operation failed: {e}");
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database operation failed".into(),
                )
            } else {
                ApiError(StatusCode::BAD_REQUEST, e.to_string())
            }
        })
    }
}
pub struct ApiError(pub StatusCode, pub String);
type ApiResult<T> = std::result::Result<T, ApiError>;
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
async fn auth(State(app): State<App>, mut req: Request, next: Next) -> ApiResult<Response> {
    if req.headers().contains_key(header::ORIGIN) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "browser origins are not accepted; use your gateway".into(),
        ));
    }
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let hash = digest(token.as_bytes());
    let p = app
        .identities
        .iter()
        .find(|(expected, _)| bool::from(expected.as_bytes().ct_eq(hash.as_bytes())))
        .map(|(_, p)| p.clone())
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "invalid credentials".into()))?;
    req.extensions_mut().insert(p);
    Ok(next.run(req).await)
}
fn role(p: &Principal, role: &str) -> ApiResult<()> {
    if p.has(role) {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            format!("{role} role required"),
        ))
    }
}
pub fn router(app: App) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/jobs", get(jobs).post(submit))
        .route("/v1/jobs/{id}", get(job))
        .route("/v1/jobs/{id}/cancel", post(cancel))
        .route("/v1/jobs/{id}/renew", post(renew))
        .route("/v1/jobs/{id}/complete", post(complete))
        .route("/v1/jobs/{id}/fail", post(fail))
        .route("/v1/workers/register", post(register))
        .route("/v1/workers/claim", post(claim))
        .route("/v1/workers/{id}/pause", post(pause))
        .route("/v1/credits", post(credit))
        .route("/v1/artifacts", post(artifact_put))
        .route("/v1/artifacts/{id}", get(artifact_get))
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .with_state(app)
}
async fn status(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    app.with(|s| {
        s.sweep(now())?;
        Ok(Json(json!({"api_version":API_VERSION,"release":"0.2.0","settlement":"test_credits",
            "principal":{"id":p.id,"roles":p.roles,"pools":p.pools,"projects":p.projects},"account":s.balance(&p.id)?,"nodes":s.nodes(&p,now())?,"jobs":s.jobs(&p)?,
            "capabilities":{"cpu_sha256":true,"embeddings":"pinned_ollama_batches","gpu_evidence":"worker_reported_full_residency","durable_storage":false,
                "temporary_artifacts":true,"public_marketplace":false,"mesh_discovery":false}})))
    })
}
async fn jobs(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Vec<Job>>> {
    app.with(|s| Ok(Json(s.jobs(&p)?)))
}
async fn submit(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Json(spec): Json<Submit>,
) -> ApiResult<Json<Job>> {
    role(&p, "submitter")?;
    app.with(|s| Ok(Json(s.submit(&p, spec, now())?)))
}
async fn job(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult<Json<Job>> {
    app.with(|s| {
        let job = s.job(&id)?;
        anyhow::ensure!(job.owner == p.id || p.has("admin"), "job not accessible");
        Ok(Json(job))
    })
}
async fn cancel(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult<Json<Job>> {
    role(&p, "submitter")?;
    app.with(|s| Ok(Json(s.cancel(&p, &id, now())?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lease {
    lease_id: String,
}
async fn renew(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(req): Json<Lease>,
) -> ApiResult<Json<Job>> {
    role(&p, "worker")?;
    app.with(|s| Ok(Json(s.renew(&p, &id, &req.lease_id, now())?)))
}
async fn complete(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(req): Json<Completion>,
) -> ApiResult<Json<Job>> {
    role(&p, "worker")?;
    app.with(|s| Ok(Json(s.complete(&p, &id, req, now())?)))
}
async fn fail(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(req): Json<Lease>,
) -> ApiResult<Json<Job>> {
    role(&p, "worker")?;
    app.with(|s| Ok(Json(s.fail(&p, &id, &req.lease_id, now())?)))
}
async fn register(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Json(offer): Json<Offer>,
) -> ApiResult<Json<Node>> {
    role(&p, "worker")?;
    app.with(|s| Ok(Json(s.register(&p, offer, now())?)))
}
async fn claim(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Option<Job>>> {
    role(&p, "worker")?;
    app.with(|s| Ok(Json(s.claim(&p, now())?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pause {
    paused: bool,
}
async fn pause(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
    Json(req): Json<Pause>,
) -> ApiResult<Json<Value>> {
    if p.id != id {
        role(&p, "admin")?;
    } else {
        role(&p, "worker")?;
    }
    app.with(|s| {
        s.pause(&id, req.paused)?;
        Ok(Json(json!({"id":id,"paused":req.paused})))
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credit {
    account: String,
    amount: u64,
    idempotency_key: String,
}
async fn credit(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Json(req): Json<Credit>,
) -> ApiResult<Json<Value>> {
    role(&p, "admin")?;
    if !app.identities.iter().any(|(_, p)| p.id == req.account) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "unknown account".into()));
    }
    app.with(|s| {
        Ok(Json(s.credit(
            &req.account,
            req.amount,
            &req.idempotency_key,
            now(),
        )?))
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    content: String,
    ttl_seconds: i64,
}
async fn artifact_put(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Json(req): Json<Artifact>,
) -> ApiResult<Json<Value>> {
    role(&p, "submitter")?;
    app.with(|s| {
        Ok(Json(s.artifact_put(
            &p,
            &req.content,
            req.ttl_seconds,
            now(),
        )?))
    })
}
async fn artifact_get(
    State(app): State<App>,
    Extension(p): Extension<Principal>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    role(&p, "submitter")?;
    app.with(|s| Ok(Json(s.artifact_get(&p, &id, now())?)))
}
