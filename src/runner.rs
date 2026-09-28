use crate::{
    model::{Language, Mode, Rep},
    process::{self, Cancel},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{process::Command, time::Duration};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub detail: String,
}
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub checks: Vec<CheckResult>,
}
impl Evaluation {
    pub fn passed(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|c| c.status == "passed")
    }
    pub fn assertion_failed(&self) -> bool {
        self.checks.iter().any(|c| c.status == "failed")
            && !self.checks.iter().any(|c| c.status == "error")
    }
}

pub trait Runner: Send + Sync {
    fn evaluate(
        &self,
        language: Language,
        code: &str,
        checks: &[(String, String)],
        cancel: &Cancel,
    ) -> Result<Evaluation>;
}

/// Apple containers on Apple silicon macOS; Docker remains an explicit alternative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerEngine {
    Apple,
    Docker,
}

impl ContainerEngine {
    pub fn configured() -> Result<Self> {
        Self::from_setting(std::env::var("SPAR_RUNNER").ok().as_deref())
    }
    pub fn from_setting(setting: Option<&str>) -> Result<Self> {
        match setting {
            None | Some("auto") => Ok(if cfg!(target_os = "macos") {
                Self::Apple
            } else {
                Self::Docker
            }),
            Some("apple") => Ok(Self::Apple),
            Some("docker") => Ok(Self::Docker),
            Some(_) => anyhow::bail!("SPAR_RUNNER must be auto, apple, or docker"),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Apple => "Apple container",
            Self::Docker => "Docker",
        }
    }
    fn executable(self) -> &'static str {
        match self {
            Self::Apple => "container",
            Self::Docker => "docker",
        }
    }
    fn command(self) -> Command {
        let mut cmd = Command::new(self.executable());
        // Do not forward CLI debug/platform overrides, SSH agents, or API credentials.
        cmd.env_clear();
        for key in [
            "PATH",
            "HOME",
            "USER",
            "TMPDIR",
            "TEMP",
            "TMP",
            "SYSTEMROOT",
            "DOCKER_HOST",
            "DOCKER_CONTEXT",
            "DOCKER_CONFIG",
            "DOCKER_TLS_VERIFY",
            "DOCKER_CERT_PATH",
        ] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("NO_COLOR", "1");
        cmd
    }
    pub fn available(self, language: Language, cancel: &Cancel) -> Result<()> {
        if self == Self::Apple {
            ensure!(
                cfg!(all(target_os = "macos", target_arch = "aarch64")),
                "Apple container requires an Apple silicon Mac. Set SPAR_RUNNER=docker on other platforms."
            );
        }
        let help = process::run(
            self.command().args(["run", "--help"]),
            None,
            Duration::from_secs(10),
            cancel,
        )
        .with_context(|| {
            format!(
                "Install {}{}; then run `spar setup`",
                self.name(),
                if self == Self::Apple {
                    " with `brew install container`"
                } else {
                    " and start it"
                }
            )
        })?;
        ensure!(help.code == Some(0), "{} CLI is unavailable", self.name());
        for flag in [
            "--network",
            "--read-only",
            "--cap-drop",
            "--user",
            "--memory",
            "--cpus",
            "--ulimit",
            "--mount",
            "--tmpfs",
        ] {
            ensure!(
                help.text.contains(flag),
                "Update {}: missing {flag}",
                self.name()
            );
        }
        self.require_image(language, cancel)
    }
    fn require_image(self, language: Language, cancel: &Cancel) -> Result<()> {
        let result = process::run(
            self.command().args(["image", "inspect", language.image()]),
            None,
            Duration::from_secs(10),
            cancel,
        )?;
        ensure!(
            result.code == Some(0),
            "{} runner image or service unavailable. Run `spar setup`.",
            self.name()
        );
        Ok(())
    }
    pub fn setup(self, cancel: &Cancel) -> Result<()> {
        if self == Self::Apple {
            ensure!(
                cfg!(all(target_os = "macos", target_arch = "aarch64")),
                "Apple container requires an Apple silicon Mac"
            );
            println!("Starting Apple container and preparing its Linux kernel…");
            let result = process::run(
                self.command()
                    .args(["system", "start", "--enable-kernel-install"]),
                None,
                Duration::from_secs(600),
                cancel,
            )
            .context(
                "Install Apple container with `brew install container`, then retry `spar setup`",
            )?;
            ensure!(
                result.code == Some(0),
                "Apple container service could not start. Run `container system start --enable-kernel-install` for details."
            );
        }
        for language in [Language::Python, Language::Typescript] {
            println!("Preparing {} runner with {}…", language, self.name());
            let mut command = self.command();
            match self {
                Self::Apple => {
                    command.args([
                        "image",
                        "pull",
                        "--platform",
                        "linux/arm64",
                        "--progress",
                        "none",
                        language.image(),
                    ]);
                }
                Self::Docker => {
                    command.args(["pull", language.image()]);
                }
            }
            let result = process::run(&mut command, None, Duration::from_secs(300), cancel)?;
            ensure!(
                result.code == Some(0),
                "{} image download failed. Check the service and your connection.",
                self.name()
            );
            // Warm the VM init image and validate the real execution path at setup time.
            let code = match language {
                Language::Python => "def solve(value): return value",
                Language::Typescript => "export function solve(value: any) { return value; }",
            };
            let test = match language {
                Language::Python => "assert solve(42) == 42",
                Language::Typescript => "assert.equal(solve(42), 42);",
            };
            ensure!(
                self.evaluate_with_timeout(
                    language,
                    code,
                    &[("Runner setup".into(), test.into())],
                    cancel,
                    Duration::from_secs(300)
                )?
                .passed(),
                "{} runner setup check failed",
                language
            );
        }
        Ok(())
    }
}

pub fn container_args(
    engine: ContainerEngine,
    path: &std::path::Path,
    language: Language,
    name: &str,
) -> Vec<String> {
    let mut args = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        name.into(),
        "--network=none".into(),
        "--read-only".into(),
        "--cap-drop=ALL".into(),
        "--user=65534:65534".into(),
        "--cpus=1".into(),
        "--ulimit=nofile=64:64".into(),
    ];
    match engine {
        ContainerEngine::Apple => args.extend(
            [
                "--platform=linux/arm64",
                "--progress=none",
                "--no-dns",
                "--memory=512m",
                "--ulimit=nproc=32:32",
            ]
            .map(String::from),
        ),
        ContainerEngine::Docker => args.extend(
            [
                "--pull=never",
                "--security-opt=no-new-privileges",
                "--memory=128m",
                "--memory-swap=128m",
                "--pids-limit=32",
                "--log-driver=none",
            ]
            .map(String::from),
        ),
    }
    args.extend(
        [
            "--tmpfs=/tmp:rw,noexec,nosuid,size=16m",
            "--workdir=/rep",
            "--mount",
        ]
        .map(String::from),
    );
    args.push(format!(
        "type=bind,source={},target=/rep,readonly",
        path.display()
    ));
    args.push(language.image().into());
    // BusyBox's launcher sets no_new_privs before loading any exercise code.
    if engine == ContainerEngine::Apple {
        args.extend(["/bin/busybox", "setpriv", "--nnp"].map(String::from));
    }
    match language {
        Language::Python => args.extend(["python", "-B", "-u", "harness.py"].map(String::from)),
        Language::Typescript => args.extend(
            [
                "node",
                "--disable-warning=ExperimentalWarning",
                "harness.ts",
            ]
            .map(String::from),
        ),
    }
    args
}

impl Runner for ContainerEngine {
    fn evaluate(
        &self,
        language: Language,
        code: &str,
        checks: &[(String, String)],
        cancel: &Cancel,
    ) -> Result<Evaluation> {
        self.evaluate_with_timeout(language, code, checks, cancel, Duration::from_secs(30))
    }
}

impl ContainerEngine {
    fn evaluate_with_timeout(
        &self,
        language: Language,
        code: &str,
        checks: &[(String, String)],
        cancel: &Cancel,
        timeout: Duration,
    ) -> Result<Evaluation> {
        process::cancelled(cancel)?;
        self.require_image(language, cancel)?;
        ensure!(
            code.len() <= 32_000 && checks.iter().all(|(_, c)| c.len() <= 32_000),
            "Workspace file exceeds 32 KB"
        );
        let dir = tempfile::tempdir()?;
        // The unprivileged container user needs traversal of this isolated snapshot only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::write(dir.path().join(language.file()), code)?;
        let harness = match language {
            Language::Python => format!(
                "{}\nrun({})\n",
                include_str!("../runners/harness.py"),
                serde_json::to_string(&serde_json::to_string(checks)?)?
            ),
            Language::Typescript => {
                let mut text = include_str!("../runners/harness.ts").to_string();
                for (name, source) in checks {
                    text.push_str(&format!(
                        "\nawait check({}, async () => {{\n{}\n}});\n",
                        serde_json::to_string(name)?,
                        source
                    ));
                }
                text.push_str("\nrawOut('\\nSPAR_RESULTS:' + JSON.stringify(results) + '\\n');\n");
                text
            }
        };
        let file = if language == Language::Python {
            "harness.py"
        } else {
            "harness.ts"
        };
        std::fs::write(dir.path().join(file), harness)?;
        // Native Node type stripping applies to both the solution and test harness.
        std::fs::write(dir.path().join("package.json"), "{\"type\":\"module\"}")?;
        let name = format!("spar-{}", uuid::Uuid::new_v4());
        let result = process::run(
            self.command()
                .args(container_args(*self, dir.path(), language, &name)),
            None,
            timeout,
            cancel,
        );
        // Killing the CLI alone does not stop its container. Always remove by generated name.
        let _ = process::run(
            self.command().args(["rm", "--force", &name]),
            None,
            Duration::from_secs(10),
            &process::cancel_token(),
        );
        // Explicit cancellation is control flow, not a failed learner check.
        // Cleanup must finish before returning; timeouts still retain diagnostics.
        process::cancelled(cancel)?;
        let output = match result {
            Ok(output) => output,
            Err(error) => {
                if let Some(interrupted) = error.downcast_ref::<process::Interrupted>() {
                    return Ok(execution_error(
                        checks,
                        &interrupted.reason,
                        &interrupted.stdout,
                        &interrupted.stderr,
                    ));
                }
                return Err(error.context("Could not start execution"));
            }
        };
        if output.code != Some(0) {
            return Ok(execution_error(
                checks,
                "Execution exited unsuccessfully",
                &output.stdout,
                &output.stderr,
            ));
        }
        let line = output
            .stdout
            .lines()
            .rev()
            .find_map(|s| s.strip_prefix("SPAR_RESULTS:"));
        let Some(line) = line else {
            return Ok(execution_error(
                checks,
                "Runner produced no results (early exit or excessive output)",
                &output.stdout,
                &output.stderr,
            ));
        };
        let results: Vec<CheckResult> =
            serde_json::from_str(line).context("Runner produced invalid results")?;
        ensure!(
            results.len() == checks.len()
                && results.iter().zip(checks).all(|(a, b)| a.name == b.0
                    && ["passed", "failed", "error"].contains(&a.status.as_str())),
            "Runner result mismatch"
        );
        Ok(Evaluation { checks: results })
    }
}

fn execution_error(
    checks: &[(String, String)],
    detail: &str,
    stdout: &str,
    stderr: &str,
) -> Evaluation {
    Evaluation {
        checks: checks
            .iter()
            .enumerate()
            .map(|(i, (name, _))| CheckResult {
                name: name.clone(),
                status: "error".into(),
                detail: detail.into(),
                stdout: if i == 0 {
                    stdout.chars().take(16_000).collect()
                } else {
                    String::new()
                },
                stderr: if i == 0 {
                    stderr.chars().take(16_000).collect()
                } else {
                    String::new()
                },
            })
            .collect(),
    }
}

fn acceptance(rep: &Rep) -> Vec<(String, String)> {
    rep.checks
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (
                format!("{} · check {}", c.requirement, i + 1),
                c.code.clone(),
            )
        })
        .collect()
}

fn examples(rep: &Rep) -> Vec<(String, String)> {
    rep.examples
        .iter()
        .enumerate()
        .map(|(i, example)| {
            let input = serde_json::to_string(&example.input_json).expect("string JSON encoding");
            let output = serde_json::to_string(&example.output_json).expect("string JSON encoding");
            let code = match rep.language {
                Language::Python => format!(
                    "import json\nassert solve(json.loads({input})) == json.loads({output})"
                ),
                Language::Typescript => {
                    format!("assert.deepEqual(solve(JSON.parse({input})), JSON.parse({output}));")
                }
            };
            (format!("Example {}", i + 1), code)
        })
        .collect()
}

/// Admission uses reference checks, expected starter behavior, and mapped negative checks.
pub fn validate(rep: &Rep, runner: &dyn Runner, cancel: &Cancel) -> Result<()> {
    rep.validate()?;
    let checks = acceptance(rep);
    ensure!(
        runner
            .evaluate(rep.language, &rep.reference, &checks, cancel)?
            .passed(),
        "Reference failed acceptance checks"
    );
    let mut visible = vec![("Visible examples".into(), rep.visible_tests.clone())];
    visible.extend(examples(rep));
    ensure!(
        runner
            .evaluate(rep.language, &rep.reference, &visible, cancel)?
            .passed(),
        "Reference failed visible examples"
    );
    let starter = runner.evaluate(rep.language, &rep.starter, &checks, cancel)?;
    if rep.mode == Mode::Test {
        ensure!(
            starter.passed() && rep.starter == rep.reference,
            "Test rep must start with the correct implementation"
        );
    } else {
        ensure!(
            starter.assertion_failed(),
            "Starter must fail an assertion, with no runtime errors"
        );
    }
    if rep.mode == Mode::Test || (rep.version == 1 && rep.mode == Mode::Debug) {
        ensure!(
            runner
                .evaluate(rep.language, &rep.starter, &visible, cancel)?
                .passed(),
            "Starter must pass visible examples"
        );
    }
    let reference_tests = vec![("Reference test suite".into(), rep.reference_tests.clone())];
    ensure!(
        runner
            .evaluate(rep.language, &rep.reference, &reference_tests, cancel)?
            .passed(),
        "Reference tests rejected correct behavior"
    );
    if rep.mode == Mode::Debug {
        ensure!(
            runner
                .evaluate(rep.language, &rep.starter, &reference_tests, cancel)?
                .assertion_failed(),
            "Reference regression tests must catch the starter bug"
        );
    }
    let mut missed = false;
    for mutant in &rep.mutants {
        let mapped = rep
            .checks
            .iter()
            .filter(|c| c.requirement == mutant.requirement)
            .map(|c| (c.requirement.clone(), c.code.clone()))
            .collect::<Vec<_>>();
        ensure!(
            runner
                .evaluate(rep.language, &mutant.code, &mapped, cancel)?
                .assertion_failed(),
            "Negative check failed to catch the intended bug by assertion"
        );
        ensure!(
            runner
                .evaluate(rep.language, &mutant.code, &reference_tests, cancel)?
                .assertion_failed(),
            "Reference test suite did not detect a mutant"
        );
        missed |= runner
            .evaluate(rep.language, &mutant.code, &visible, cancel)?
            .passed();
    }
    if rep.mode == Mode::Test {
        ensure!(missed, "Starter tests already catch every mutant");
    }
    Ok(())
}

pub fn assess(
    rep: &Rep,
    code: &str,
    tests: &str,
    runner: &dyn Runner,
    cancel: &Cancel,
) -> Result<Evaluation> {
    let empty_tests = tests.trim().is_empty();
    if empty_tests && rep.mode == Mode::Test {
        return Ok(Evaluation { checks: vec![CheckResult {
            name: "Write your own tests".into(), status: "failed".into(),
            detail: "The Tests editor is empty. Add assertions using solve(value) that distinguish correct behavior from bugs.".into(),
            ..Default::default()
        }] });
    }
    let learner = vec![("Your tests".into(), tests.to_string())];
    if rep.mode == Mode::Test {
        let mut result = runner.evaluate(rep.language, &rep.reference, &learner, cancel)?;
        if !result.passed() {
            return Ok(result);
        }
        for (i, m) in rep.mutants.iter().enumerate() {
            let caught = runner
                .evaluate(rep.language, &m.code, &learner, cancel)?
                .assertion_failed();
            result.checks.push(CheckResult {
                name: format!("Catch bug {} ({})", i + 1, m.requirement),
                status: if caught { "passed" } else { "failed" }.into(),
                ..Default::default()
            });
        }
        return Ok(result);
    }
    let mut checks = acceptance(rep);
    checks.extend(examples(rep));
    if !empty_tests {
        checks.extend(learner.clone());
    }
    let mut result = runner.evaluate(rep.language, code, &checks, cancel)?;
    if empty_tests && rep.mode == Mode::Debug {
        result.checks.push(CheckResult {
            name: "Add a regression test".into(), status: "failed".into(),
            detail: "Write a test in Tests that catches the original bug. Examples are provided in the brief.".into(),
            ..Default::default()
        });
        return Ok(result);
    }
    if rep.mode == Mode::Debug && result.passed() {
        let correct = runner
            .evaluate(rep.language, &rep.reference, &learner, cancel)?
            .passed();
        let caught = runner
            .evaluate(rep.language, &rep.starter, &learner, cancel)?
            .assertion_failed();
        result.checks.push(CheckResult {
            name: "Regression test detects the bug and accepts correct behavior".into(),
            status: if correct && caught {
                "passed"
            } else {
                "failed"
            }
            .into(),
            ..Default::default()
        });
    }
    Ok(result)
}
