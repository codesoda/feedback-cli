use std::future::Future;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State as AxumState;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::watch;
use tower_http::trace::TraceLayer;

use crate::assets;
use crate::events::EventEmitter;
use crate::sse::EventBus;
use crate::state::{SharedState, State};
use crate::{render, template, DiscussError, Result};

const JAVASCRIPT_CONTENT_TYPE: &str = "application/javascript";
const ASSET_CACHE_CONTROL: &str = "public, max-age=86400";
const SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct AppState {
    pub state: SharedState,
    pub bus: Arc<EventBus>,
    pub emitter: Arc<EventEmitter>,
    markdown_source: Arc<str>,
    shutdown: ShutdownSignal,
}

impl AppState {
    pub fn new(state: SharedState, bus: Arc<EventBus>, emitter: Arc<EventEmitter>) -> Self {
        Self {
            state,
            bus,
            emitter,
            markdown_source: Arc::from(""),
            shutdown: ShutdownSignal::new(),
        }
    }

    pub fn for_process() -> Self {
        Self::new(
            State::new_shared(),
            Arc::new(EventBus::new(1024)),
            Arc::new(EventEmitter::stdout()),
        )
    }

    pub fn with_markdown_source(mut self, markdown_source: impl Into<String>) -> Self {
        let markdown_source = markdown_source.into();
        self.markdown_source = Arc::from(markdown_source.into_boxed_str());
        self
    }

    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::for_process()
    }
}

#[derive(Clone, Debug)]
struct ShutdownSignal {
    tx: watch::Sender<bool>,
}

impl ShutdownSignal {
    fn new() -> Self {
        let (tx, _) = watch::channel(false);

        Self { tx }
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }

    fn signal(&self) {
        self.tx.send_replace(true);
    }
}

pub async fn serve<F>(addr: SocketAddr, app_state: AppState, shutdown: F) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    ensure_loopback(addr)?;

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|error| bind_error(addr, error))?;
    let router = build_router(app_state.clone());
    let shutdown_signal = app_state.shutdown.clone();

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.await;
            shutdown_signal.signal();
        })
        .await
        .map_err(|source| DiscussError::ServerBindError { addr, source })
}

fn build_router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(get_root))
        .route("/api/state", get(get_api_state))
        .route("/api/events", get(get_api_events))
        .route("/assets/mermaid.min.js", get(get_mermaid_js))
        .route("/assets/mermaid-shim.js", get(get_mermaid_shim_js))
        .fallback(not_found)
        .layer(TraceLayer::new_for_http())
        .with_state(app_state)
}

async fn get_root(AxumState(app_state): AxumState<AppState>) -> Response {
    match render_root_page(&app_state) {
        Ok(page) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            page,
        )
            .into_response(),
        Err(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into_response(),
    }
}

fn render_root_page(app_state: &AppState) -> std::result::Result<String, String> {
    let snapshot = app_state
        .state
        .read()
        .map_err(|_| "state lock poisoned while rendering page".to_string())?
        .snapshot();
    let initial_state_json = serde_json::to_string(&snapshot)
        .map_err(|error| format!("failed to serialize initial state: {error}"))?;
    let rendered_markdown = render::render(app_state.markdown_source.as_ref());

    Ok(template::render_page(
        &rendered_markdown,
        &initial_state_json,
    ))
}

async fn get_api_state(AxumState(app_state): AxumState<AppState>) -> Response {
    match app_state.state.read() {
        Ok(state) => Json(state.snapshot()).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "state lock poisoned while reading state",
        )
            .into_response(),
    }
}

async fn get_api_events(AxumState(app_state): AxumState<AppState>) -> impl IntoResponse {
    let mut events = app_state.bus.subscribe();
    let mut shutdown = app_state.subscribe_shutdown();
    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                biased;

                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                event = events.recv() => {
                    match event {
                        Ok(event) => {
                            let Ok(payload) = serde_json::to_string(&event.payload) else {
                                continue;
                            };
                            yield Ok::<_, std::convert::Infallible>(
                                SseEvent::default().event(event.kind).data(payload),
                            );
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(SSE_HEARTBEAT_INTERVAL)
            .text("keep-alive"),
    )
}

async fn get_mermaid_js() -> impl IntoResponse {
    javascript_response(assets::mermaid_js())
}

async fn get_mermaid_shim_js() -> impl IntoResponse {
    javascript_response(assets::mermaid_shim_js())
}

fn javascript_response(body: &'static str) -> impl IntoResponse {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, JAVASCRIPT_CONTENT_TYPE),
            (header::CACHE_CONTROL, ASSET_CACHE_CONTROL),
        ],
        body,
    )
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

fn ensure_loopback(addr: SocketAddr) -> Result<()> {
    if addr.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Ok(());
    }

    Err(DiscussError::ServerBindError {
        addr,
        source: io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "discuss only binds to 127.0.0.1",
        ),
    })
}

fn bind_error(addr: SocketAddr, error: io::Error) -> DiscussError {
    if error.kind() == io::ErrorKind::AddrInUse {
        DiscussError::PortInUse { port: addr.port() }
    } else {
        DiscussError::ServerBindError {
            addr,
            source: error,
        }
    }
}
