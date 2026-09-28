use anyhow::{Context, Result, bail};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub type Cancel = Arc<AtomicBool>;
pub fn cancel_token() -> Cancel {
    Arc::new(AtomicBool::new(false))
}
pub fn cancelled(cancel: &Cancel) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Cancelled");
    }
    Ok(())
}

#[derive(Debug)]
pub struct Output {
    pub code: Option<i32>,
    pub text: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub struct Interrupted {
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
}
impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}
impl std::error::Error for Interrupted {}

fn capture(mut stream: impl Read) -> String {
    let mut output = Vec::new();
    let mut buf = [0; 4096];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        let left = 128_000usize.saturating_sub(output.len());
        output.extend_from_slice(&buf[..n.min(left)]);
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Drain both pipes concurrently, bound captured output, and terminate the process group.
pub fn run(
    cmd: &mut Command,
    input: Option<&str>,
    timeout: Duration,
    cancel: &Cancel,
) -> Result<Output> {
    cancelled(cancel)?;
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().context("Could not start child process")?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || capture(stdout));
    let err = thread::spawn(move || capture(stderr));
    let mut stdin = child.stdin.take().unwrap();
    let input = input.unwrap_or_default().to_owned();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let start = Instant::now();
    let result = loop {
        if let Some(status) = child.try_wait()? {
            break Ok(status.code());
        }
        if cancel.load(Ordering::Relaxed) {
            break Err(anyhow::anyhow!("Cancelled"));
        }
        if start.elapsed() > timeout {
            break Err(anyhow::anyhow!("Operation timed out"));
        }
        thread::sleep(Duration::from_millis(40));
    };
    // Also remove descendants that kept the pipes open after the parent exited.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = writer.join();
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            return Err(Interrupted {
                reason: error.to_string(),
                stdout,
                stderr,
            }
            .into());
        }
    };
    Ok(Output {
        code,
        text: format!("{stdout}{stderr}"),
        stdout,
        stderr,
    })
}
