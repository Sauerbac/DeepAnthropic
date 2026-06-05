//! cc-router — a tiny Anthropic-format reverse proxy for one blended Claude Code session.
//!
//! Routes by the `model` field in the request body:
//!   * model contains "opus"  -> api.anthropic.com   (your Pro plan, OAuth Bearer + oauth beta)
//!   * model contains "haiku" -> api.deepseek.com/anthropic  (x-api-key, model -> deepseek-v4-flash)
//!   * anything else (sonnet) -> api.deepseek.com/anthropic  (x-api-key, model -> deepseek-v4-pro)
//!
//! Both upstreams speak the Anthropic wire format, so there is NO body translation:
//! we swap auth + (for DeepSeek) the model name, then stream the SSE response straight back.

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
struct Config {
    #[serde(default = "d_bind")]
    bind: String,
    #[serde(default = "d_port")]
    port: u16,
    anthropic: Anthropic,
    deepseek: Deepseek,
}

#[derive(Deserialize)]
struct Anthropic {
    #[serde(default = "d_anthropic_url")]
    base_url: String,
    /// Long-lived subscription token from `claude setup-token` (sk-ant-oat01-...).
    oauth_token: String,
    #[serde(default = "d_oauth_beta")]
    oauth_beta: String,
}

#[derive(Deserialize)]
struct Deepseek {
    #[serde(default = "d_deepseek_url")]
    base_url: String,
    api_key: String,
    #[serde(default = "d_pro")]
    pro_model: String,
    #[serde(default = "d_flash")]
    flash_model: String,
}

fn d_bind() -> String {
    "127.0.0.1".into()
}
fn d_port() -> u16 {
    8788
}
fn d_anthropic_url() -> String {
    "https://api.anthropic.com".into()
}
fn d_oauth_beta() -> String {
    "oauth-2025-04-20".into()
}
fn d_deepseek_url() -> String {
    "https://api.deepseek.com/anthropic".into()
}
fn d_pro() -> String {
    "deepseek-v4-pro".into()
}
fn d_flash() -> String {
    "deepseek-v4-flash".into()
}

struct AppState {
    cfg: Config,
    client: reqwest::Client,
}

enum Route {
    Anthropic,
    DeepseekPro,
    DeepseekFlash,
}

fn pick_route(model: &str) -> Route {
    let m = model.to_ascii_lowercase();
    if m.contains("opus") {
        Route::Anthropic
    } else if m.contains("haiku") {
        Route::DeepseekFlash
    } else {
        // sonnet, or anything unrecognised, runs on the cheap-but-capable tier.
        Route::DeepseekPro
    }
}

/// Request headers we never forward upstream (auth is re-applied per route;
/// the rest are hop-by-hop or recomputed by reqwest).
fn skip_req_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "authorization"
            | "x-api-key"
            | "accept-encoding"
            | "connection"
            | "proxy-authorization"
            | "anthropic-beta"
            | "transfer-encoding"
    )
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let cfg_text = std::fs::read_to_string("config.toml")
        .expect("config.toml not found next to the executable (copy config.toml.example)");
    let cfg: Config = toml::from_str(&cfg_text).expect("invalid config.toml");

    let bind = cfg.bind.clone();
    let port = cfg.port;
    let state = Arc::new(AppState {
        cfg,
        client: reqwest::Client::new(),
    });

    let app = Router::new().fallback(handler).with_state(state);
    let addr = format!("{bind}:{port}");
    tracing::info!("cc-router listening on http://{addr}");
    tracing::info!("point Claude Code at it:  $env:ANTHROPIC_BASE_URL = \"http://{addr}\"");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let cfg = &state.cfg;

    // Pull the model out of the JSON body (best-effort; non-JSON requests fall through).
    let model = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("model")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        });

    let route = match &model {
        Some(m) => pick_route(m),
        None => Route::DeepseekPro,
    };

    let (base_url, new_model): (&str, Option<&str>) = match route {
        Route::Anthropic => (&cfg.anthropic.base_url, None),
        Route::DeepseekPro => (&cfg.deepseek.base_url, Some(&cfg.deepseek.pro_model)),
        Route::DeepseekFlash => (&cfg.deepseek.base_url, Some(&cfg.deepseek.flash_model)),
    };

    // For DeepSeek routes, rewrite the outgoing model name in the body.
    let mut out_body: Vec<u8> = body.to_vec();
    if let Some(nm) = new_model {
        if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&body) {
            if v.get("model").is_some() {
                v["model"] = serde_json::Value::String(nm.to_string());
                if let Ok(b) = serde_json::to_vec(&v) {
                    out_body = b;
                }
            }
        }
    }

    let path_and_query = uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or_else(|| uri.path());
    let url = format!("{}{}", base_url.trim_end_matches('/'), path_and_query);

    tracing::info!(
        "{} {} -> {}",
        method,
        model.as_deref().unwrap_or("<no-model>"),
        new_model.unwrap_or("anthropic:opus-passthrough")
    );

    let rmethod = reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap();
    let mut req = state.client.request(rmethod, &url).body(out_body);

    // Capture the inbound anthropic-beta so we can merge/forward it deliberately.
    let incoming_beta = headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Forward all non-skipped headers verbatim (content-type, anthropic-version, user-agent, ...).
    for (name, value) in headers.iter() {
        let n = name.as_str();
        if skip_req_header(n) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            req = req.header(n, v);
        }
    }

    // Apply the per-route auth.
    match route {
        Route::Anthropic => {
            // OAuth subscription token: Bearer + ensure the oauth beta flag is present.
            let beta = match &incoming_beta {
                Some(b) if b.contains("oauth") => b.clone(),
                Some(b) => format!("{},{}", b, cfg.anthropic.oauth_beta),
                None => cfg.anthropic.oauth_beta.clone(),
            };
            req = req
                .header(
                    "authorization",
                    format!("Bearer {}", cfg.anthropic.oauth_token),
                )
                .header("anthropic-beta", beta);
        }
        _ => {
            req = req.header("x-api-key", &cfg.deepseek.api_key);
            if let Some(b) = &incoming_beta {
                req = req.header("anthropic-beta", b);
            }
        }
    }

    // Send and stream the response back unbuffered (SSE).
    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("upstream error: {e}");
            return (StatusCode::BAD_GATEWAY, format!("cc-router upstream error: {e}"))
                .into_response();
        }
    };

    let status = upstream.status().as_u16();
    let mut builder = Response::builder().status(status);
    for (name, value) in upstream.headers().iter() {
        let n = name.as_str();
        // Let axum frame the streamed body itself.
        if matches!(
            n,
            "content-length" | "transfer-encoding" | "connection" | "content-encoding"
        ) {
            continue;
        }
        builder = builder.header(n, value.as_bytes());
    }

    builder
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap()
}
