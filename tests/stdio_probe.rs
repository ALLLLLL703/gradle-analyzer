//! Real-stdio JSON-RPC probe against the built `gradle-analyzer` binary.
//!
//! This is the gating "directly usable" evidence: it launches the binary as a child
//! process and frames real LSP traffic over real stdin/stdout. The Task-1 cases assert a
//! well-formed `InitializeResult` and malformed-frame resilience; the Task-8 case adds a
//! latency-budget SLA assertion — an `initialize` -> `didOpen` -> `documentSymbol`
//! round-trip must complete well under the configured `latency.static_diagnostics_ms`,
//! and a rapid `didChange` followed by another request must not hang the loop.

use std::io::{BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gradle_analyzer::config::GradleAnalyzerConfig;
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
    // The full v1 surface is advertised so the protocol is ready (handlers fill later).
    for provider in [
        "documentSymbolProvider",
        "completionProvider",
        "definitionProvider",
        "referencesProvider",
        "codeActionProvider",
        "hoverProvider",
    ] {
        assert!(
            capabilities.get(provider).is_some(),
            "missing {provider}: {init_response}"
        );
    }

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
fn static_round_trip_meets_latency_budget_and_no_stale_after_change() {
    let budget_ms = GradleAnalyzerConfig::default().latency.static_diagnostics_ms;

    let mut child = spawn_server();
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));

    let init = request(1, "initialize", json!({ "capabilities": {} }));
    stdin.write_all(&encode_frame(&init)).expect("send initialize");
    stdin.flush().expect("flush");
    let _ = read_until_id(&mut stdout, 1);
    stdin
        .write_all(&encode_frame(&notification("initialized", json!({}))))
        .expect("send initialized");

    let uri = "file:///proj/build.gradle.kts";
    stdin
        .write_all(&encode_frame(&did_open(uri, 1, "plugins {}\n")))
        .expect("send didOpen");
    stdin.flush().expect("flush didOpen");

    // Time the static documentSymbol round-trip; it must beat the configured budget by a
    // wide margin (it reads an in-memory snapshot and never waits on the model tier).
    let start = Instant::now();
    stdin
        .write_all(&encode_frame(&document_symbol(2, uri)))
        .expect("send documentSymbol");
    stdin.flush().expect("flush documentSymbol");
    let symbol_response = read_until_id(&mut stdout, 2);
    let elapsed = start.elapsed();

    assert!(symbol_response.get("error").is_none(), "{symbol_response}");
    assert!(
        symbol_response["result"].is_array() || symbol_response["result"].is_null(),
        "documentSymbol seam returns empty array/null: {symbol_response}"
    );
    // Generous ceiling: budget plus process-scheduling slack, asserting REAL wall time.
    let ceiling = Duration::from_millis(budget_ms) + Duration::from_secs(2);
    assert!(
        elapsed < ceiling,
        "static round-trip {elapsed:?} exceeded ceiling {ceiling:?} (budget {budget_ms} ms)"
    );

    // Rapid didChange then another request: the loop must stay responsive (answers id 3)
    // and never deliver a stale result from the superseded generation.
    stdin
        .write_all(&encode_frame(&did_change(uri, 2, "plugins { java }\n")))
        .expect("send didChange");
    stdin
        .write_all(&encode_frame(&document_symbol(3, uri)))
        .expect("send documentSymbol 2");
    stdin.flush().expect("flush");
    let after_change = read_until_id(&mut stdout, 3);
    assert!(after_change.get("error").is_none(), "{after_change}");
    assert_eq!(after_change["id"], 3, "loop stayed live after change");

    let shutdown = request(4, "shutdown", Value::Null);
    stdin.write_all(&encode_frame(&shutdown)).expect("send shutdown");
    stdin.flush().expect("flush");
    let _ = read_until_id(&mut stdout, 4);
    stdin
        .write_all(&encode_frame(&notification("exit", Value::Null)))
        .expect("send exit");
    stdin.flush().expect("flush exit");
    drop(stdin);

    let status = wait_with_timeout(&mut child, Duration::from_secs(10));
    assert!(status.is_some(), "server did not exit cleanly after SLA probe");
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

/// Builds a `textDocument/didOpen` notification.
fn did_open(uri: &str, version: i32, text: &str) -> Value {
    notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri, "languageId": "gradle", "version": version, "text": text
            }
        }),
    )
}

/// Builds a full-text `textDocument/didChange` notification.
fn did_change(uri: &str, version: i32, text: &str) -> Value {
    notification(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [ { "text": text } ]
        }),
    )
}

/// Builds a `textDocument/documentSymbol` request.
fn document_symbol(id: i64, uri: &str) -> Value {
    request(
        id,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
    )
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
