//! The real [`ProcessRunner`]: a tokio child process speaking the Task-4 framed line
//! protocol over stdio.
//!
//! This is the production transport that Task 4 deferred behind the [`ProcessRunner`]
//! trait. It spawns the JVM sidecar (planned by [`crate::gradle::sidecar::launch`]),
//! writes request lines to the child's stdin, reads `\n`-terminated reply lines from its
//! stdout, and kills the child on teardown. The child's stderr is inherited so the Gradle
//! daemon's own logging never contaminates the stdout protocol channel.
//!
//! Everything is async and non-blocking (tokio `Child` + `BufReader`), so the LSP event
//! loop is never stalled; the [`crate::gradle::sidecar::client::SidecarClient`] layers the
//! config-backed timeout and cancellation on top via `tokio::time` and a cancel token.

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::gradle::sidecar::launch::LaunchPlan;
use crate::gradle::sidecar::runner::{ProcessRunner, RunnerError};

/// A program + argument vector to spawn, decoupled from how it was planned.
///
/// Built from a [`LaunchPlan`] in production, or directly with a test double command
/// (`cat`, `sh -c '...'`) so the runner is exercised end-to-end with no JVM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// The executable to spawn.
    pub program: String,
    /// The argument vector passed to the executable.
    pub args: Vec<String>,
}

impl CommandSpec {
    /// Builds a spec from a planned sidecar launch.
    pub fn from_plan(plan: LaunchPlan) -> Self {
        Self {
            program: plan.program.to_string_lossy().into_owned(),
            args: plan.args,
        }
    }

    /// Builds a spec directly (used by tests for `cat`/`sh -c` process doubles).
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

/// A real child-process [`ProcessRunner`] over piped stdio.
///
/// Created with [`WrapperRunner::new`]; the child is not launched until
/// [`ProcessRunner::spawn`]. Reads are line-buffered; `read_line` returns `Ok(None)` at a
/// clean end-of-stream (the child exited). [`ProcessRunner::kill`] terminates the child and
/// is idempotent, so the client's degrade-and-kill paths never error on an already-dead
/// child.
///
/// # Example
///
/// ```no_run
/// # async fn run() {
/// use gradle_analyzer::gradle::sidecar::wrapper_runner::{CommandSpec, WrapperRunner};
/// use gradle_analyzer::gradle::sidecar::runner::ProcessRunner;
///
/// // A `cat` echoes whatever we write — handy for transport smoke tests.
/// let mut runner = WrapperRunner::new(CommandSpec::new("cat", vec![]));
/// runner.spawn().await.unwrap();
/// runner.write_line("{\"type\":\"hello\"}").await.unwrap();
/// let echoed = runner.read_line().await.unwrap();
/// assert_eq!(echoed.as_deref(), Some("{\"type\":\"hello\"}"));
/// runner.kill().await.unwrap();
/// # }
/// ```
pub struct WrapperRunner {
    spec: CommandSpec,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl WrapperRunner {
    /// Creates a runner for `spec` without spawning anything yet.
    pub fn new(spec: CommandSpec) -> Self {
        Self {
            spec,
            child: None,
            stdin: None,
            stdout: None,
        }
    }
}

impl ProcessRunner for WrapperRunner {
    async fn spawn(&mut self) -> Result<(), RunnerError> {
        tracing::info!(program = %self.spec.program, "spawning sidecar child");
        let mut child = Command::new(&self.spec.program)
            .args(&self.spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherit stderr: the Gradle daemon's logs go to our stderr, never to the
            // stdout protocol channel.
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| RunnerError::Spawn(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RunnerError::Spawn("child stdin not piped".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RunnerError::Spawn("child stdout not piped".to_string()))?;

        self.stdin = Some(stdin);
        self.stdout = Some(BufReader::new(stdout));
        self.child = Some(child);
        Ok(())
    }

    async fn write_line(&mut self, line: &str) -> Result<(), RunnerError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| RunnerError::Write("sidecar not spawned".to_string()))?;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| RunnerError::Write(e.to_string()))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| RunnerError::Write(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| RunnerError::Write(e.to_string()))?;
        Ok(())
    }

    async fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        let stdout = self
            .stdout
            .as_mut()
            .ok_or_else(|| RunnerError::Read("sidecar not spawned".to_string()))?;
        let mut line = String::new();
        let read = stdout
            .read_line(&mut line)
            .await
            .map_err(|e| RunnerError::Read(e.to_string()))?;
        if read == 0 {
            // Clean end-of-stream: the child exited.
            return Ok(None);
        }
        Ok(Some(line))
    }

    async fn kill(&mut self) -> Result<(), RunnerError> {
        if let Some(mut child) = self.child.take() {
            // Best-effort: a child that already exited is not an error.
            let _ = child.start_kill();
            let _ = child.wait().await;
            tracing::info!("sidecar child terminated");
        }
        self.stdin = None;
        self.stdout = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cat` echoes a written line straight back — proves write→read framing round-trips.
    #[tokio::test]
    async fn cat_echoes_a_written_line() {
        let mut runner = WrapperRunner::new(CommandSpec::new("cat", vec![]));
        runner.spawn().await.expect("spawn cat");

        runner.write_line("{\"type\":\"hello\"}").await.expect("write");
        let echoed = runner.read_line().await.expect("read");
        assert_eq!(echoed.as_deref(), Some("{\"type\":\"hello\"}\n"));

        runner.kill().await.expect("kill");
    }

    /// A child that emits one line then exits reports the line, then a clean EOF.
    #[tokio::test]
    async fn child_emitting_one_line_then_exiting_reports_eof() {
        let mut runner = WrapperRunner::new(CommandSpec::new(
            "sh",
            vec!["-c".to_string(), "printf 'one\\n'".to_string()],
        ));
        runner.spawn().await.expect("spawn");

        assert_eq!(runner.read_line().await.unwrap().as_deref(), Some("one\n"));
        assert_eq!(runner.read_line().await.unwrap(), None, "clean EOF after exit");

        runner.kill().await.expect("kill");
    }

    /// Spawning a nonexistent program is a recoverable [`RunnerError::Spawn`], not a panic.
    #[tokio::test]
    async fn spawning_a_missing_program_is_a_spawn_error() {
        let mut runner = WrapperRunner::new(CommandSpec::new(
            "ga-no-such-binary-xyz",
            vec![],
        ));
        let err = runner.spawn().await.unwrap_err();
        assert!(matches!(err, RunnerError::Spawn(_)));
    }

    /// `kill` is idempotent and safe before/after spawn.
    #[tokio::test]
    async fn kill_is_idempotent() {
        let mut runner = WrapperRunner::new(CommandSpec::new("cat", vec![]));
        runner.kill().await.expect("kill before spawn is a no-op");
        runner.spawn().await.expect("spawn");
        runner.kill().await.expect("first kill");
        runner.kill().await.expect("second kill is a no-op");
    }
}
