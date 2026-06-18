//! The [`ProcessRunner`] transport seam and an in-process [`FakeRunner`] for tests.
//!
//! A [`ProcessRunner`] is the *only* side-effecting boundary of the sidecar: it spawns the
//! process, writes request lines, reads reply lines, and kills the process. Keeping it a
//! narrow async trait lets [`crate::gradle::sidecar::client::SidecarClient`] drive the
//! entire protocol against a scripted [`FakeRunner`] with no JVM and no real child process.
//!
//! The real wrapper-launching implementation is **Task 14**. It will live behind this same
//! trait (a `WrapperRunner` spawning `./gradlew` with the Tooling-API init script and
//! speaking the line-delimited JSON contract). Task 4 deliberately ships only the fake so
//! the contract is fully proven first; see the `// Task 14 seam` note below.

use std::collections::VecDeque;
use std::time::Duration;

use crate::gradle::sidecar::protocol::{ServerHello, ServerMessage, SidecarResponse};

/// A transport-level error from the runner (spawn/write/read), distinct from a protocol
/// or model failure.
///
/// The client maps these onto the user-facing [`crate::gradle::sidecar::SidecarFailure`]
/// taxonomy; clean end-of-stream is reported as `Ok(None)` from
/// [`ProcessRunner::read_line`] rather than an error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunnerError {
    /// The process could not be spawned.
    #[error("failed to spawn sidecar process: {0}")]
    Spawn(String),
    /// A line could not be written to the process.
    #[error("failed to write to sidecar: {0}")]
    Write(String),
    /// A line could not be read from the process.
    #[error("failed to read from sidecar: {0}")]
    Read(String),
}

/// The async transport contract for talking to a sidecar process.
///
/// Implementors own the process lifecycle. All methods are async and non-blocking so the
/// LSP event loop is never stalled. `read_line` returns `Ok(None)` on a clean
/// end-of-stream (the process exited), which the client treats as an early exit.
///
/// # Task 14 seam
///
/// The production implementation launching the Gradle wrapper is added in Task 14 behind
/// this trait. Task 4 ships only [`FakeRunner`]; do not launch a real JVM here.
pub trait ProcessRunner {
    /// Starts the underlying process (a no-op for [`FakeRunner`]).
    fn spawn(&mut self) -> impl std::future::Future<Output = Result<(), RunnerError>> + Send;

    /// Writes one frame line (the `\n` terminator is appended by the implementor).
    fn write_line(
        &mut self,
        line: &str,
    ) -> impl std::future::Future<Output = Result<(), RunnerError>> + Send;

    /// Reads the next frame line, or `Ok(None)` at a clean end-of-stream.
    fn read_line(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<String>, RunnerError>> + Send;

    /// Terminates the process and releases its resources.
    fn kill(&mut self) -> impl std::future::Future<Output = Result<(), RunnerError>> + Send;
}

/// One scripted read the [`FakeRunner`] serves, optionally after a delay.
#[derive(Debug, Clone)]
struct ReadStep {
    delay: Option<Duration>,
    payload: ReadPayload,
}

/// What a scripted read yields.
#[derive(Debug, Clone)]
enum ReadPayload {
    /// A literal reply line (may be malformed on purpose).
    Line(String),
    /// A clean end-of-stream (the "process exited early" case).
    Eof,
    /// A read that never completes (the "process hangs" case).
    Hang,
}

/// An in-process [`ProcessRunner`] whose reads are scripted ahead of time.
///
/// Build one with [`FakeRunner::builder`]. Each `read_line` call serves the next scripted
/// step — a line, a clean EOF, or a never-completing hang — optionally after a delay
/// (which under `tokio::test(start_paused = true)` advances virtual time deterministically).
/// Everything the client writes is recorded in [`FakeRunner::written`] for assertions.
///
/// # Example
///
/// ```
/// use gradle_analyzer::gradle::sidecar::runner::FakeRunner;
/// use gradle_analyzer::gradle::sidecar::protocol::{Capability, ServerHello};
///
/// let runner = FakeRunner::builder()
///     .hello(ServerHello { chosen_version: 1, capabilities: vec![Capability::ModelImport] })
///     .hang()
///     .build();
/// assert!(runner.written().is_empty());
/// ```
pub struct FakeRunner {
    steps: VecDeque<ReadStep>,
    written: Vec<String>,
    spawned: bool,
    killed: bool,
}

impl FakeRunner {
    /// Starts a builder for a scripted runner.
    pub fn builder() -> FakeRunnerBuilder {
        FakeRunnerBuilder {
            steps: VecDeque::new(),
        }
    }

    /// The lines the client has written so far, in order.
    pub fn written(&self) -> &[String] {
        &self.written
    }

    /// Whether [`ProcessRunner::spawn`] was called.
    pub fn was_spawned(&self) -> bool {
        self.spawned
    }

    /// Whether [`ProcessRunner::kill`] was called.
    pub fn was_killed(&self) -> bool {
        self.killed
    }
}

impl ProcessRunner for FakeRunner {
    async fn spawn(&mut self) -> Result<(), RunnerError> {
        self.spawned = true;
        Ok(())
    }

    async fn write_line(&mut self, line: &str) -> Result<(), RunnerError> {
        self.written.push(line.to_string());
        Ok(())
    }

    async fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        let step = match self.steps.pop_front() {
            Some(step) => step,
            // An exhausted script behaves like a clean end-of-stream.
            None => return Ok(None),
        };
        if let Some(delay) = step.delay {
            tokio::time::sleep(delay).await;
        }
        match step.payload {
            ReadPayload::Line(line) => Ok(Some(line)),
            ReadPayload::Eof => Ok(None),
            ReadPayload::Hang => std::future::pending().await,
        }
    }

    async fn kill(&mut self) -> Result<(), RunnerError> {
        self.killed = true;
        Ok(())
    }
}

/// Builds a [`FakeRunner`] by appending scripted read steps in reply order.
pub struct FakeRunnerBuilder {
    steps: VecDeque<ReadStep>,
}

impl FakeRunnerBuilder {
    /// Queues a server handshake reply.
    pub fn hello(self, hello: ServerHello) -> Self {
        self.message(ServerMessage::Hello(hello))
    }

    /// Queues a request response.
    pub fn response(self, response: SidecarResponse) -> Self {
        self.message(ServerMessage::Response(Box::new(response)))
    }

    /// Queues an already-typed server message, serialized to a JSON line.
    pub fn message(self, message: ServerMessage) -> Self {
        let line = serde_json::to_string(&message).expect("server message serializes");
        self.line(line)
    }

    /// Queues a raw reply line (use to inject a malformed/non-JSON frame).
    pub fn line(mut self, line: impl Into<String>) -> Self {
        self.steps.push_back(ReadStep {
            delay: None,
            payload: ReadPayload::Line(line.into()),
        });
        self
    }

    /// Queues a raw reply line served only after `delay`.
    pub fn line_after(mut self, delay: Duration, line: impl Into<String>) -> Self {
        self.steps.push_back(ReadStep {
            delay: Some(delay),
            payload: ReadPayload::Line(line.into()),
        });
        self
    }

    /// Queues a clean end-of-stream (the process exits early).
    pub fn eof(mut self) -> Self {
        self.steps.push_back(ReadStep {
            delay: None,
            payload: ReadPayload::Eof,
        });
        self
    }

    /// Queues a read that never completes (the process hangs).
    pub fn hang(mut self) -> Self {
        self.steps.push_back(ReadStep {
            delay: None,
            payload: ReadPayload::Hang,
        });
        self
    }

    /// Finalizes the scripted [`FakeRunner`].
    pub fn build(self) -> FakeRunner {
        FakeRunner {
            steps: self.steps,
            written: Vec::new(),
            spawned: false,
            killed: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gradle::sidecar::protocol::Capability;

    #[tokio::test]
    async fn fake_runner_serves_scripted_lines_then_eof() {
        let mut runner = FakeRunner::builder()
            .hello(ServerHello {
                chosen_version: 1,
                capabilities: vec![Capability::ModelImport],
            })
            .build();

        runner.spawn().await.unwrap();
        runner.write_line("{\"type\":\"hello\"}").await.unwrap();

        let first = runner.read_line().await.unwrap();
        assert!(first.unwrap().contains("\"chosenVersion\":1"));

        // Script exhausted -> clean EOF.
        assert_eq!(runner.read_line().await.unwrap(), None);
        assert_eq!(runner.written(), &["{\"type\":\"hello\"}".to_string()]);

        runner.kill().await.unwrap();
        assert!(runner.was_spawned() && runner.was_killed());
    }

    #[tokio::test]
    async fn explicit_eof_step_reports_early_exit() {
        let mut runner = FakeRunner::builder().eof().build();
        assert_eq!(runner.read_line().await.unwrap(), None);
    }
}
