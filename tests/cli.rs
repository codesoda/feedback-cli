use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::DateTime;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn cli_busy_port_exits_three_and_reports_port() {
    let busy_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind busy listener");
    let busy_port = busy_listener
        .local_addr()
        .expect("busy listener addr")
        .port();
    let env_port = free_port();
    let temp_dir = tempdir().expect("tempdir should be created");
    let home_dir = temp_dir.path().join("home");
    fs::create_dir(&home_dir).expect("home dir should be created");
    let markdown_path = temp_dir.path().join("review.md");
    fs::write(&markdown_path, "# Review\n").expect("markdown file should be written");

    let child = Command::new(env!("CARGO_BIN_EXE_discuss"))
        .arg("--port")
        .arg(busy_port.to_string())
        .arg(&markdown_path)
        .env("HOME", &home_dir)
        .env("DISCUSS_PORT", env_port.to_string())
        .env_remove("DISCUSS_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn discuss binary");
    let output = wait_with_timeout(child, Duration::from_secs(2));

    assert_eq!(output.status.code(), Some(3));
    assert!(
        output.stdout.is_empty(),
        "stdout should be reserved for JSON events"
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(stderr.contains(&format!("port {busy_port}")));
    assert!(stderr.contains("pass --port <N>"));
    assert!(stderr.contains("stop the other instance"));
}

#[test]
fn cli_no_open_logs_listening_url_to_stderr() {
    let port = free_port();
    let temp_dir = tempdir().expect("tempdir should be created");
    let home_dir = temp_dir.path().join("home");
    fs::create_dir(&home_dir).expect("home dir should be created");
    let markdown_path = temp_dir.path().join("review.md");
    fs::write(&markdown_path, "# Review\n").expect("markdown file should be written");

    let mut child = Command::new(env!("CARGO_BIN_EXE_discuss"))
        .arg("--no-open")
        .arg("--port")
        .arg(port.to_string())
        .arg(&markdown_path)
        .env("HOME", &home_dir)
        .env_remove("DISCUSS_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn discuss binary");
    let stderr = child.stderr.take().expect("stderr pipe should be present");
    let line_rx = read_first_line(stderr);

    let line = line_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("listening line should be written")
        .expect("stderr line should be readable");
    assert_eq!(line, format!("listening on http://127.0.0.1:{port}\n"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn cli_emits_single_session_started_event_after_listening() {
    let port = free_port();
    let temp_dir = tempdir().expect("tempdir should be created");
    let home_dir = temp_dir.path().join("home");
    fs::create_dir(&home_dir).expect("home dir should be created");
    let markdown_path = temp_dir.path().join("review.md");
    fs::write(&markdown_path, "# Review\n").expect("markdown file should be written");
    let source_file = fs::canonicalize(&markdown_path)
        .expect("markdown path should canonicalize")
        .to_string_lossy()
        .into_owned();

    let mut child = Command::new(env!("CARGO_BIN_EXE_discuss"))
        .arg("--no-open")
        .arg("--port")
        .arg(port.to_string())
        .arg(&markdown_path)
        .env("HOME", &home_dir)
        .env_remove("DISCUSS_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn discuss binary");
    let stderr = child.stderr.take().expect("stderr pipe should be present");
    let line_rx = read_first_line(stderr);

    let line = line_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("listening line should be written")
        .expect("stderr line should be readable");
    assert_eq!(line, format!("listening on http://127.0.0.1:{port}\n"));

    let output = kill_and_collect(child);
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("stdout line should be JSON"))
        .collect::<Vec<_>>();

    assert_eq!(events.len(), 1, "stdout should contain one startup event");
    let event = &events[0];
    assert_eq!(event["kind"], "session.started");
    assert_rfc3339(event["at"].as_str().expect("event at should be a string"));
    assert_eq!(event["payload"]["url"], format!("http://127.0.0.1:{port}"));
    assert_eq!(event["payload"]["source_file"], source_file);
    assert_rfc3339(
        event["payload"]["started_at"]
            .as_str()
            .expect("started_at should be a string"),
    );
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind free listener");

    listener.local_addr().expect("free listener addr").port()
}

fn read_first_line<R>(reader: R) -> mpsc::Receiver<io::Result<String>>
where
    R: Read + Send + 'static,
{
    let (line_tx, line_rx) = mpsc::channel();

    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = line_tx.send(result);
    });

    line_rx
}

fn kill_and_collect(mut child: Child) -> Output {
    let _ = child.kill();
    child.wait_with_output().expect("collect child output")
}

fn assert_rfc3339(value: &str) {
    DateTime::parse_from_rfc3339(value).expect("timestamp should be RFC3339");
}

fn wait_with_timeout(mut child: Child, duration: Duration) -> Output {
    let deadline = Instant::now() + duration;

    loop {
        if child.try_wait().expect("poll child").is_some() {
            return child.wait_with_output().expect("collect child output");
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect timed out output");
            panic!(
                "discuss did not exit within {:?}; stdout: {}; stderr: {}",
                duration,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        thread::sleep(Duration::from_millis(10));
    }
}
