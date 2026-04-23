use std::future::pending;
use std::net::{Ipv4Addr, SocketAddr, TcpListener as StdTcpListener};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use discuss::state::{Thread, ThreadId, ThreadKind};
use discuss::{serve, AppState, DiscussError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn get_root_renders_template_and_shutdown_completes() {
    let addr = free_loopback_addr();
    let app_state = AppState::for_process().with_markdown_source("# Review Plan\n\nBody text.");
    let mut shutdown_rx = app_state.subscribe_shutdown();
    let (shutdown_tx, shutdown_rx_signal) = oneshot::channel();
    let server = tokio::spawn(serve(addr, app_state, async move {
        let _ = shutdown_rx_signal.await;
    }));

    wait_for_server(addr).await;

    let response = get_root(addr).await;
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response
        .to_ascii_lowercase()
        .contains("content-type: text/html; charset=utf-8"));
    assert!(doc_content(response_body(&response)).contains("<h1>Review Plan</h1>"));

    shutdown_tx.send(()).expect("send shutdown signal");
    timeout(Duration::from_secs(1), shutdown_rx.changed())
        .await
        .expect("shutdown signal within timeout")
        .expect("shutdown sender still active");
    assert!(*shutdown_rx.borrow());

    timeout(Duration::from_secs(1), server)
        .await
        .expect("server exits within timeout")
        .expect("server task should not panic")
        .expect("server shutdown should succeed");
}

#[tokio::test]
async fn shutdown_allows_started_request_to_complete() {
    let addr = free_loopback_addr();
    let app_state = AppState::for_process().with_markdown_source("# Started Request");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve(addr, app_state, async move {
        let _ = shutdown_rx.await;
    }));

    wait_for_server(addr).await;

    let mut stream = TcpStream::connect(addr)
        .await
        .expect("connect before shutdown");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request before shutdown");
    sleep(Duration::from_millis(20)).await;

    shutdown_tx.send(()).expect("send shutdown signal");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    assert!(response.starts_with("HTTP/1.1 200"));

    timeout(Duration::from_secs(1), server)
        .await
        .expect("server exits within timeout")
        .expect("server task should not panic")
        .expect("server shutdown should succeed");
}

#[tokio::test]
async fn get_root_seeds_current_state_for_reload() {
    let addr = free_loopback_addr();
    let app_state = AppState::for_process().with_markdown_source("# State Seed");
    {
        let mut state = app_state
            .state
            .write()
            .expect("state lock should not be poisoned");
        state.add_thread(thread("u-one", 1));
        state.add_thread(thread("u-two", 4));
    }
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(serve(addr, app_state, async move {
        let _ = shutdown_rx.await;
    }));

    wait_for_server(addr).await;

    let response = get_root(addr).await;
    let initial_state = initial_state_script(response_body(&response));

    assert!(initial_state.contains("\"u-one\""));
    assert!(initial_state.contains("\"u-two\""));
    assert!(doc_content(response_body(&response)).contains("<h1>State Seed</h1>"));

    shutdown_tx.send(()).expect("send shutdown signal");
    timeout(Duration::from_secs(1), server)
        .await
        .expect("server exits within timeout")
        .expect("server task should not panic")
        .expect("server shutdown should succeed");
}

#[tokio::test]
async fn busy_port_maps_to_port_in_use() {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind busy listener");
    let addr = listener.local_addr().expect("busy listener addr");

    let error = serve(addr, AppState::for_process(), pending())
        .await
        .expect_err("busy port should fail");

    assert!(matches!(
        error,
        DiscussError::PortInUse { port } if port == addr.port()
    ));
}

#[tokio::test]
async fn rejects_non_loopback_bind_addr() {
    let addr = SocketAddr::from(([0, 0, 0, 0], 0));

    let error = serve(addr, AppState::for_process(), pending())
        .await
        .expect_err("public bind addr should fail");

    assert!(matches!(
        error,
        DiscussError::ServerBindError { addr: rejected, .. } if rejected == addr
    ));
}

fn free_loopback_addr() -> SocketAddr {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("allocate free port");
    listener.local_addr().expect("free listener addr")
}

async fn wait_for_server(addr: SocketAddr) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        match TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(error) if tokio::time::Instant::now() < deadline => {
                let _ = error;
                sleep(Duration::from_millis(10)).await;
            }
            Err(error) => panic!("server did not start at {addr}: {error}"),
        }
    }
}

async fn get_root(addr: SocketAddr) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect to server");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .expect("write request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    response
}

fn timestamp(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 23, 2, 30, second)
        .single()
        .expect("valid timestamp")
}

fn thread(id: &str, anchor_start: usize) -> Thread {
    Thread {
        id: ThreadId(id.to_string()),
        anchor_start,
        anchor_end: anchor_start + 1,
        snippet: format!("snippet {id}"),
        breadcrumb: "Overview".to_string(),
        text: format!("thread {id}"),
        created_at: timestamp(0),
        kind: ThreadKind::User,
    }
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("http response should contain a body separator")
}

fn doc_content(body: &str) -> &str {
    let open = "<section id=\"doc-content\">";
    let close = "</section>";
    let start = body.find(open).expect("doc-content start") + open.len();
    let end = body[start..].find(close).expect("doc-content end") + start;

    &body[start..end]
}

fn initial_state_script(body: &str) -> &str {
    let open = "<script id=\"discuss-initial-state\">";
    let close = "</script>";
    let start = body.find(open).expect("initial-state script start") + open.len();
    let end = body[start..].find(close).expect("initial-state script end") + start;

    &body[start..end]
}
