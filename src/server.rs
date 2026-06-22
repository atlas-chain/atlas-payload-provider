use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::frontend::INDEX_HTML;
use crate::model::PayloadSubmission;
use crate::store::{PayloadStore, StoreFailure, SubmitOutcome};
use crate::validation;

#[derive(Clone, Debug)]
pub struct AppState {
    pub store: Arc<PayloadStore>,
    pub payload_dir: Arc<String>,
    pub html_title: Arc<String>,
    pub ingress_key: Option<Arc<String>>,
}

pub async fn run_server(state: AppState, listen_host: String, listen_port: u16) {
    let bind_address = format!("{listen_host}:{listen_port}");

    let listener = match TcpListener::bind(&bind_address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("failed to bind HTTP server on {bind_address}: {error}");
            return;
        }
    };

    let snapshot = state.store.snapshot(8);
    println!(
        "{}",
        json!({
            "message": "atlas payload provider listening",
            "url": format!("http://{bind_address}/payloads"),
            "ui": format!("http://{bind_address}/"),
            "payloadDir": state.payload_dir.as_str(),
            "payloadCount": snapshot.payload_count,
            "maxPayloadBytes": snapshot.max_payload_bytes,
            "ingressProtected": state.ingress_key.is_some(),
            "signingEnabled": state.store.signer_address().is_some(),
            "signerAddress": state.store.signer_address(),
            "endpoints": ["/", "/status", "/payloads", "/payloads/{id}", "/payloads/{id}/raw", "/healthz"],
        })
    );

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/status", get(status_handler))
        .route("/payloads", get(list_payloads).post(submit_payload))
        .route("/payloads/{id}", get(get_payload))
        .route("/payloads/{id}/raw", get(get_payload_raw))
        .route("/healthz", get(health_handler))
        .fallback(not_found_handler)
        .with_state(state);

    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("HTTP server failed: {error}");
    }
}

async fn index_handler(State(state): State<AppState>) -> Response {
    let html = render_index_html(state.html_title.as_str());

    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

async fn health_handler(State(state): State<AppState>) -> Json<Value> {
    let snapshot = state.store.snapshot(0);
    Json(json!({
        "ok": true,
        "payloadCount": snapshot.payload_count,
        "totalBytes": snapshot.total_bytes,
    }))
}

async fn status_handler(State(state): State<AppState>) -> Json<Value> {
    let snapshot = state.store.snapshot(25);
    Json(json!({
        "ok": true,
        "service": "atlas-payload-provider",
        "payloadDir": state.payload_dir.as_str(),
        "payloadCount": snapshot.payload_count,
        "totalBytes": snapshot.total_bytes,
        "maxPayloadBytes": snapshot.max_payload_bytes,
        "ingressProtected": state.ingress_key.is_some(),
        "signingEnabled": state.store.signer_address().is_some(),
        "signerAddress": state.store.signer_address(),
        "latest": snapshot.latest,
        "endpoints": ["/", "/status", "/payloads", "/payloads/{id}", "/payloads/{id}/raw", "/healthz"],
    }))
}

async fn list_payloads(State(state): State<AppState>) -> Json<Value> {
    let snapshot = state.store.snapshot(100);
    Json(json!({
        "ok": true,
        "payloadCount": snapshot.payload_count,
        "payloads": snapshot.latest,
    }))
}

async fn submit_payload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(submission): Json<PayloadSubmission>,
) -> Response {
    if let Err(response) = authorize_ingress(&state, &headers) {
        return *response;
    }

    let started = Instant::now();
    let validated =
        match validation::validate_submission(submission, state.store.max_payload_bytes()) {
            Ok(validated) => validated,
            Err(error) => return error_response(StatusCode::BAD_REQUEST, error.to_string()),
        };

    match state.store.submit(validated) {
        Ok(outcome) => publish_submission(started, outcome),
        Err(StoreFailure::Persistence(message)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
        Err(StoreFailure::Signing(message)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

async fn get_payload(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(record) = state.store.get(&id) else {
        return error_response(StatusCode::NOT_FOUND, format!("payload {id} not found"));
    };

    (
        StatusCode::OK,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-cache"),
        ],
        Json(json!({
            "ok": true,
            "payload": record,
        })),
    )
        .into_response()
}

async fn get_payload_raw(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(record) = state.store.get(&id) else {
        return error_response(StatusCode::NOT_FOUND, format!("payload {id} not found"));
    };

    let bytes = match STANDARD.decode(record.payload_base64.as_bytes()) {
        Ok(bytes) => bytes,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("stored payload {id} could not be decoded: {error}"),
            );
        }
    };

    let mut response = Body::from(bytes).into_response();
    response.headers_mut().insert(CACHE_CONTROL, no_cache());
    response.headers_mut().insert(
        CONTENT_TYPE,
        content_type_header(record.content_type.as_deref()),
    );
    response
}

async fn not_found_handler() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "ok": false, "error": { "message": "Not found" } })),
    )
        .into_response()
}

fn authorize_ingress(state: &AppState, headers: &HeaderMap) -> Result<(), Box<Response>> {
    let Some(key) = state.ingress_key.as_ref() else {
        return Ok(());
    };

    let Some(provided) = bearer_token(headers) else {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "missing or malformed Authorization header",
        )));
    };

    if !constant_time_eq(provided.as_bytes(), key.as_bytes()) {
        return Err(Box::new(error_response(
            StatusCode::UNAUTHORIZED,
            "invalid bearer key",
        )));
    }

    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("authorization")?.to_str().ok()?;
    let value = raw.strip_prefix("Bearer ")?;
    Some(value.trim().to_string())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn render_index_html(title: &str) -> String {
    INDEX_HTML.replace("__HTML_TITLE__", &escape_html(title))
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn publish_submission(started: Instant, outcome: SubmitOutcome) -> Response {
    let latency_ms = started.elapsed().as_millis();
    let status = if outcome.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    println!(
        "{}",
        json!({
            "message": if outcome.created { "payload receipt issued" } else { "payload receipt confirmed" },
            "path": "/payloads",
            "status": status.as_u16(),
            "id": outcome.record.id,
            "namespace": outcome.record.namespace,
            "sizeBytes": outcome.record.size_bytes,
            "checksum": outcome.record.checksum,
            "signerAddress": outcome.record.signature.as_ref().map(|signature| signature.signer.as_str()),
            "latencyMs": latency_ms.to_string(),
        })
    );

    (
        status,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-cache"),
        ],
        Json(json!({
            "ok": true,
            "created": outcome.created,
            "payload": outcome.record,
        })),
    )
        .into_response()
}

fn no_cache() -> HeaderValue {
    HeaderValue::from_static("no-cache")
}

fn content_type_header(content_type: Option<&str>) -> HeaderValue {
    content_type
        .and_then(|value| HeaderValue::from_str(value).ok())
        .unwrap_or_else(|| HeaderValue::from_static("application/octet-stream"))
}

fn error_response<S>(status: StatusCode, message: S) -> Response
where
    S: AsRef<str>,
{
    (
        status,
        Json(json!({ "ok": false, "error": { "message": message.as_ref() } })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::ValidatedPayload;

    fn temp_payload_dir(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "atlas-payload-server-{name}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        path
    }

    fn state_with_key(key: Option<&str>) -> AppState {
        let dir = temp_payload_dir("state");
        AppState {
            store: Arc::new(PayloadStore::load(dir.clone(), 1024, None).expect("store")),
            payload_dir: Arc::new(dir.to_string_lossy().to_string()),
            html_title: Arc::new("Atlas Payload Provider".to_string()),
            ingress_key: key.map(|k| Arc::new(k.to_string())),
        }
    }

    async fn body_string(response: Response) -> (StatusCode, axum::http::HeaderMap, String) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let body = String::from_utf8(bytes.to_vec()).expect("utf8 body");
        (status, headers, body)
    }

    #[test]
    fn constant_time_eq_handles_inputs() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn bearer_token_parses_header() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer s3cret".parse().unwrap());
        assert_eq!(bearer_token(&headers).as_deref(), Some("s3cret"));
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn index_handler_serves_html() {
        let state = state_with_key(None);
        let (status, headers, body) = body_string(index_handler(State(state)).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers.get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert!(body.contains("Atlas Payload Provider"));
    }

    #[tokio::test]
    async fn index_handler_escapes_configured_title() {
        let mut state = state_with_key(None);
        state.html_title = Arc::new("Payload <Provider> & \"Ingress\"".to_string());

        let (_, _, body) = body_string(index_handler(State(state)).await).await;

        assert!(body.contains("<title>Payload &lt;Provider&gt; &amp; &quot;Ingress&quot;</title>"));
        assert!(body.contains("<h1>Payload &lt;Provider&gt; &amp; &quot;Ingress&quot;</h1>"));
        assert!(!body.contains("<title>Payload <Provider>"));
    }

    #[tokio::test]
    async fn health_handler_reports_status() {
        let state = state_with_key(None);
        let Json(body) = health_handler(State(state)).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["payloadCount"], json!(0));
    }

    #[tokio::test]
    async fn submit_is_open_without_key() {
        let state = state_with_key(None);
        let submission = PayloadSubmission {
            namespace: "atlas.blocks".to_string(),
            content_type: None,
            payload_base64: "aGVsbG8=".to_string(),
        };

        let response = submit_payload(State(state), HeaderMap::new(), Json(submission)).await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn submit_rejects_wrong_bearer() {
        let state = state_with_key(Some("real-key"));
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        let submission = PayloadSubmission {
            namespace: "atlas.blocks".to_string(),
            content_type: None,
            payload_base64: "aGVsbG8=".to_string(),
        };

        let response = submit_payload(State(state), headers, Json(submission)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_payload_raw_serves_stored_bytes() {
        let state = state_with_key(None);
        let outcome = state
            .store
            .submit(ValidatedPayload {
                namespace: "atlas.blocks".to_string(),
                content_type: Some("text/plain".to_string()),
                bytes: b"hello".to_vec(),
            })
            .unwrap();

        let response = get_payload_raw(State(state), Path(outcome.record.id)).await;
        let status = response.status();
        let content_type = response.headers().get(CONTENT_TYPE).cloned();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.unwrap(), "text/plain");
        assert_eq!(bytes.as_ref(), b"hello");
    }
}
