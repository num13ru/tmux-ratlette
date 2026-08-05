use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_ERROR_DETAIL_CHARS: usize = 240;

pub(crate) fn run(command: &str) -> Result<Vec<u8>, String> {
    run_with_options("/bin/sh", command, DEFAULT_TIMEOUT, MAX_CAPTURE_BYTES)
}

fn run_with_options(
    shell: &str,
    command_text: &str,
    timeout: Duration,
    max_capture_bytes: usize,
) -> Result<Vec<u8>, String> {
    let mut command = Command::new(shell);
    command
        .arg("-c")
        .arg(command_text)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {shell}: {error}"))?;
    let process_group = child.id();
    let Some(stdout) = child.stdout.take() else {
        terminate(&mut child, process_group);
        return Err("could not capture plugin stdout".to_owned());
    };
    let Some(stderr) = child.stderr.take() else {
        terminate(&mut child, process_group);
        return Err("could not capture plugin stderr".to_owned());
    };
    let captured = Arc::new(AtomicUsize::new(0));
    let (events, receiver) = mpsc::channel();
    let stdout_reader = spawn_reader(
        stdout,
        Stream::Stdout,
        Arc::clone(&captured),
        max_capture_bytes,
        events.clone(),
    )
    .map_err(|error| {
        terminate(&mut child, process_group);
        format!("could not start plugin stdout reader: {error}")
    })?;
    let stderr_reader =
        match spawn_reader(stderr, Stream::Stderr, captured, max_capture_bytes, events) {
            Ok(reader) => reader,
            Err(error) => {
                terminate(&mut child, process_group);
                let _ = stdout_reader.join();
                return Err(format!("could not start plugin stderr reader: {error}"));
            }
        };
    let started = Instant::now();

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                kill_process_group(process_group);
                break status;
            }
            Ok(None) => {}
            Err(error) => {
                terminate(&mut child, process_group);
                join_readers(stdout_reader, stderr_reader)?;
                return Err(format!("could not inspect plugin command: {error}"));
            }
        }

        let elapsed = started.elapsed();
        if elapsed >= timeout {
            terminate(&mut child, process_group);
            join_readers(stdout_reader, stderr_reader)?;
            return Err(format!(
                "plugin command timed out after {}",
                format_duration(timeout)
            ));
        }
        let wait = POLL_INTERVAL.min(timeout.saturating_sub(elapsed));
        match receiver.recv_timeout(wait) {
            Ok(ReaderEvent::LimitExceeded(stream)) => {
                terminate(&mut child, process_group);
                join_readers(stdout_reader, stderr_reader)?;
                return Err(format!(
                    "plugin {stream} exceeded the {} capture limit",
                    format_bytes(max_capture_bytes)
                ));
            }
            Ok(ReaderEvent::ReadFailed(stream, error)) => {
                terminate(&mut child, process_group);
                join_readers(stdout_reader, stderr_reader)?;
                return Err(format!("could not read plugin {stream}: {error}"));
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
    };

    let (stdout, stderr) = join_readers(stdout_reader, stderr_reader)?;
    if let Some(error) = stdout.error {
        return Err(format!("could not read plugin stdout: {error}"));
    }
    if let Some(error) = stderr.error {
        return Err(format!("could not read plugin stderr: {error}"));
    }
    if let Some(stream) = stdout
        .exceeded
        .then_some(Stream::Stdout)
        .or_else(|| stderr.exceeded.then_some(Stream::Stderr))
    {
        return Err(format!(
            "plugin {stream} exceeded the {} capture limit",
            format_bytes(max_capture_bytes)
        ));
    }
    exit_result(status, stdout.bytes, stderr.bytes)
}

#[derive(Debug, Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

impl std::fmt::Display for Stream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdout => formatter.write_str("stdout"),
            Self::Stderr => formatter.write_str("stderr"),
        }
    }
}

enum ReaderEvent {
    LimitExceeded(Stream),
    ReadFailed(Stream, String),
}

struct Capture {
    bytes: Vec<u8>,
    exceeded: bool,
    error: Option<String>,
}

fn spawn_reader<R>(
    mut reader: R,
    stream: Stream,
    captured: Arc<AtomicUsize>,
    max_capture_bytes: usize,
    events: mpsc::Sender<ReaderEvent>,
) -> io::Result<thread::JoinHandle<Capture>>
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name(format!("plugin-{stream}-reader"))
        .spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 8192];
            loop {
                let count = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        let error = error.to_string();
                        let _ = events.send(ReaderEvent::ReadFailed(stream, error.clone()));
                        return Capture {
                            bytes,
                            exceeded: false,
                            error: Some(error),
                        };
                    }
                };
                let previous = captured.fetch_add(count, Ordering::Relaxed);
                let remaining = max_capture_bytes.saturating_sub(previous);
                let accepted = remaining.min(count);
                bytes.extend_from_slice(&buffer[..accepted]);
                if accepted < count {
                    let _ = events.send(ReaderEvent::LimitExceeded(stream));
                    return Capture {
                        bytes,
                        exceeded: true,
                        error: None,
                    };
                }
            }
            Capture {
                bytes,
                exceeded: false,
                error: None,
            }
        })
}

fn join_readers(
    stdout: thread::JoinHandle<Capture>,
    stderr: thread::JoinHandle<Capture>,
) -> Result<(Capture, Capture), String> {
    let stdout = stdout
        .join()
        .map_err(|_| "plugin stdout reader panicked".to_owned())?;
    let stderr = stderr
        .join()
        .map_err(|_| "plugin stderr reader panicked".to_owned())?;
    Ok((stdout, stderr))
}

fn exit_result(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Result<Vec<u8>, String> {
    if status.success() {
        return Ok(stdout);
    }
    let detail = stderr_detail(&stderr);
    let status = status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| format!("exit {code}"),
    );
    if detail.is_empty() {
        Err(format!("plugin command failed with {status}"))
    } else {
        Err(format!("plugin command failed with {status}: {detail}"))
    }
}

fn stderr_detail(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let line = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    let mut characters = line.chars();
    let mut detail = characters
        .by_ref()
        .take(MAX_ERROR_DETAIL_CHARS)
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect::<String>();
    if characters.next().is_some() {
        detail.push('…');
    }
    detail
}

fn terminate(child: &mut Child, process_group: u32) {
    kill_process_group(process_group);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn kill_process_group(process_group: u32) {
    let Ok(process_group) = i32::try_from(process_group) else {
        return;
    };
    // The child is spawned into a fresh process group, so a negative PID targets
    // only that command and descendants it created, not the palette process.
    unsafe {
        libc::kill(-process_group, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process_group(_process_group: u32) {}

fn format_duration(duration: Duration) -> String {
    if duration.subsec_nanos() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes % (1024 * 1024) == 0 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else if bytes % 1024 == 0 {
        format!("{} KiB", bytes / 1024)
    } else {
        let noun = if bytes == 1 { "byte" } else { "bytes" };
        format!("{bytes} {noun}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_as_raw_bytes_without_mixing_stderr() {
        let output = run_with_options(
            "/bin/sh",
            "printf 'one\\ntwo'; printf 'diagnostic' >&2",
            Duration::from_secs(1),
            1024,
        )
        .unwrap();

        assert_eq!(output, b"one\ntwo");
    }

    #[test]
    fn preserves_non_utf8_stdout_for_the_parser() {
        let output =
            run_with_options("/bin/sh", "printf '\\377'", Duration::from_secs(1), 1024).unwrap();

        assert_eq!(output, [0xff]);
    }

    #[test]
    fn reports_nonzero_exit_with_lossy_first_stderr_line() {
        let error = run_with_options(
            "/bin/sh",
            "printf 'first line\\nsecond line' >&2; exit 7",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();

        assert_eq!(error, "plugin command failed with exit 7: first line");
    }

    #[test]
    fn sanitizes_and_bounds_stderr_details_before_display() {
        let mut stderr = b"\x1b[31m".to_vec();
        stderr.extend(std::iter::repeat_n(b'x', MAX_ERROR_DETAIL_CHARS + 10));
        stderr.extend_from_slice(b"\nignored");

        let detail = stderr_detail(&stderr);

        assert!(detail.starts_with("�[31m"));
        assert!(detail.ends_with('…'));
        assert_eq!(detail.chars().count(), MAX_ERROR_DETAIL_CHARS + 1);
        assert!(!detail.contains("ignored"));
    }

    #[test]
    fn times_out_and_terminates_the_command_process_group() {
        let started = Instant::now();
        let error = run_with_options(
            "/bin/sh",
            "sleep 5 & wait",
            Duration::from_millis(100),
            1024,
        )
        .unwrap_err();

        assert!(error.contains("timed out after 100ms"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn kills_background_descendants_that_keep_capture_pipes_open() {
        let started = Instant::now();
        let output = run_with_options(
            "/bin/sh",
            "sleep 5 & printf done",
            Duration::from_secs(1),
            1024,
        )
        .unwrap();

        assert_eq!(output, b"done");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn enforces_one_combined_limit_across_stdout_and_stderr() {
        let error = run_with_options(
            "/bin/sh",
            "while :; do printf 1234567890; printf abcdefghij >&2; done",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();

        assert!(error.contains("exceeded the 1 KiB capture limit"));
    }

    #[test]
    fn reports_shell_spawn_failures() {
        let error = run_with_options(
            "/definitely/missing/tmux-ratlette-shell",
            ":",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();

        assert!(error.contains("could not start"));
        assert!(error.contains("tmux-ratlette-shell"));
    }
}
