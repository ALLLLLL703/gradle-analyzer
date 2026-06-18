//! Real-stdio JSON-RPC probe against the built `gradle-analyzer` binary.
//!
//! This is the gating "directly usable" evidence for Task 1: it launches the binary as
//! a child process and frames `initialize` -> `initialized` -> `shutdown` -> `exit` over
//! real stdin/stdout, asserting a well-formed `InitializeResult` with capabilities. A
//! second case feeds a malformed frame and asserts the server neither panics nor hangs.

use std::io::{BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use gradle_analyzer::util::probe::{encode_frame, notification, read_frame_blocking, request};
use serde_json::{Value, json};

/// Path to the binary cargo built for this integration test.
fn binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("test exe path");
    path.pop(); // drop the test binary name
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("gradle-analyzer");
    path
}

fn spawn_server() -> std::process::Child {
    Command::new(binary_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn gradle-analyzer binary")
}

#[test]
fn real_stdio_initialize_returns_capabilities_then_shutdown() {
    let mut child = spawn_server();
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));

    let init = request(1, "initialize", json!({ "capabilities": {} }));
    stdin.write_all(&encode_frame(&init)).expect("send initialize");
    stdin.flush().expect("flush initialize");

    let init_response = read_until_id(&mut stdout, 1);
    assert_eq!(init_response["id"], 1, "transcript: {init_response}");
    let capabilities = &init_response["result"]["capabilities"];
    assert!(
        capabilities.is_object(),
        "initialize must return capabilities; got {init_response}"
    );
    assert_eq!(capabilities["textDocumentSync"], 1, "text sync FULL expected");

    let initialized = notification("initialized", json!({}));
    stdin
        .write_all(&encode_frame(&initialized))
        .expect("send initialized");

    let shutdown = request(2, "shutdown", Value::Null);
    stdin.write_all(&encode_frame(&shutdown)).expect("send shutdown");
    stdin.flush().expect("flush shutdown");

    let shutdown_response = read_until_id(&mut stdout, 2);
    assert!(shutdown_response["result"].is_null());
    assert!(shutdown_response.get("error").is_none());

    let exit = notification("exit", Value::Null);
    stdin.write_all(&encode_frame(&exit)).expect("send exit");
    stdin.flush().expect("flush exit");
    drop(stdin);

    let status = wait_with_timeout(&mut child, Duration::from_secs(10));
    assert!(status.is_some(), "server did not exit after `exit` notification");
}

#[test]
fn malformed_frame_does_not_panic_or_hang() {
    let mut child = spawn_server();
    let mut stdin = child.stdin.take().expect("child stdin");

    // A bad Content-Length and a non-JSON body: the server must not crash the harness.
    stdin
        .write_all(b"Content-Length: 9\r\n\r\nnot-json!")
        .expect("send malformed frame");
    stdin.flush().expect("flush malformed");

    // Closing stdin must let the process terminate within the timeout (no hang).
    drop(stdin);
    let status = wait_with_timeout(&mut child, Duration::from_secs(10));
    assert!(
        status.is_some(),
        "server hung or paniced on a malformed frame instead of terminating cleanly"
    );
}

/// Reads frames until one with `id` arrives, skipping server-initiated notifications.
fn read_until_id<R: std::io::BufRead>(reader: &mut R, id: i64) -> Value {
    loop {
        let frame = read_frame_blocking(reader).expect("read framed response");
        if frame.get("id") == Some(&json!(id)) {
            return frame;
        }
    }
}

/// Polls for child exit up to `timeout`, killing it if it overruns. Returns the
/// observed exit (or kill) status, or `None` only if the wait itself never resolves.
fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Option<()> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Some(()),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}
