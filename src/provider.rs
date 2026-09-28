use crate::{
    model::{Profile, Rep},
    process::{self, Cancel},
    scheduler::Objective,
};
use anyhow::{Context, Result, ensure};
use std::{process::Command, time::Duration};

pub trait Provider: Send + Sync {
    fn check(&self, cancel: &Cancel) -> Result<String>;
    fn generate(&self, profile: &Profile, objective: &Objective, cancel: &Cancel) -> Result<Rep>;
}

pub struct Codex;

/// Generation describes a complete new package. The deserialization schema also
/// accepts old cached packages and therefore has optional migration fields.
pub fn generation_schema() -> schemars::Schema {
    let mut schema = schemars::generate::SchemaSettings::default()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Rep>();
    // The empty default is solely for migrating local v1 packages. It is not
    // a valid example list for new packages and must not be suggested to Codex.
    if let Some(examples) = schema
        .as_object_mut()
        .and_then(|root| root.get_mut("properties"))
        .and_then(|properties| properties.get_mut("examples"))
        .and_then(serde_json::Value::as_object_mut)
    {
        examples.remove("default");
    }
    schema
}

#[derive(Debug)]
struct GenerationFailure {
    message: String,
    retryable: bool,
}
impl std::fmt::Display for GenerationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for GenerationFailure {}

pub(crate) fn retryable(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<GenerationFailure>()
        .is_none_or(|failure| failure.retryable)
}

/// Inspect only failure events, never agent messages or command output, which
/// can contain unrevealed teaching material. The UI receives normalized errors.
fn generation_failure(output: &process::Output) -> anyhow::Error {
    let mut failures = Vec::new();
    for line in output.stdout.lines() {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            match event["type"].as_str() {
                Some("error") => {
                    if let Some(message) = event["message"].as_str() {
                        failures.push(message.to_owned());
                    }
                }
                Some("turn.failed") => {
                    if let Some(message) = event["error"]["message"].as_str() {
                        failures.push(message.to_owned());
                    }
                }
                _ => {}
            }
        }
    }
    // stderr helps classify CLI startup/network failures, but is never displayed.
    let failure_text = format!("{}\n{}", failures.join("\n"), output.stderr).to_lowercase();
    let has = |patterns: &[&str]| {
        patterns
            .iter()
            .any(|pattern| failure_text.contains(pattern))
    };
    let (message, retryable) = if has(&[
        "invalid_json_schema",
        "invalid schema for response_format",
    ]) {
        (
            "Codex rejected Spar’s exercise schema (invalid_json_schema). Update Spar; signing in again will not fix this.",
            false,
        )
    } else if has(&[
        "usage_limit",
        "usage limit",
        "rate_limit",
        "rate limit",
        "quota",
    ]) {
        (
            "Codex allowance is unavailable. Use a cached rep or try again later.",
            false,
        )
    } else if has(&[
        "unauthorized",
        "authentication",
        "token_expired",
        "refresh_token",
        "401",
    ]) {
        (
            "Codex authentication failed. Run `codex login` with your ChatGPT account.",
            false,
        )
    } else if has(&[
        "model_not_found",
        "model is not supported",
        "unsupported model",
        "does not exist or you do not have access",
    ]) {
        (
            "Codex’s default model is unavailable for this account. Update the Codex CLI and retry.",
            false,
        )
    } else if has(&[
        "connection",
        "network",
        "dns",
        "timed out",
        "stream disconnected",
        "502",
        "503",
        "504",
    ]) {
        (
            "Codex could not reach its service or the connection was interrupted. Check your connection and retry.",
            true,
        )
    } else if has(&["invalid_request_error", "400 bad request"]) {
        (
            "Codex rejected the generation request (invalid_request_error). Update Spar and the Codex CLI.",
            false,
        )
    } else {
        (
            "Codex generation failed unexpectedly. Cached reps remain available; retry or update the Codex CLI.",
            true,
        )
    };
    GenerationFailure {
        message: message.into(),
        retryable,
    }
    .into()
}
fn command() -> Command {
    let mut cmd = Command::new("codex");
    // Explicit allowlist: preserve login location without forwarding API keys or endpoint overrides.
    cmd.env_clear();
    for key in [
        "PATH",
        "HOME",
        "USER",
        "CODEX_HOME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SYSTEMROOT",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("NO_COLOR", "1");
    cmd
}

impl Provider for Codex {
    fn check(&self, cancel: &Cancel) -> Result<String> {
        let help = process::run(
            command().args(["exec", "--help"]),
            None,
            Duration::from_secs(10),
            cancel,
        )
        .context("Install the Codex CLI and run `codex login`")?;
        ensure!(help.code == Some(0), "Codex CLI is unavailable");
        for flag in [
            "--ignore-user-config",
            "--skip-git-repo-check",
            "--sandbox",
            "--ephemeral",
            "--json",
            "--output-schema",
            "--output-last-message",
        ] {
            ensure!(help.text.contains(flag), "Update Codex CLI: missing {flag}");
        }
        let status = process::run(
            command().args(["login", "status"]),
            None,
            Duration::from_secs(10),
            cancel,
        )?;
        ensure!(
            status.code == Some(0),
            "Run `codex login` with your ChatGPT account"
        );
        ensure!(
            status.text.to_lowercase().contains("chatgpt")
                && !status.text.to_lowercase().contains("api key"),
            "Spar requires ChatGPT authentication. API-key billing is not enabled. Choose ChatGPT with `codex login` before generating."
        );
        Ok("Codex CLI · ChatGPT login".into())
    }
    fn generate(&self, profile: &Profile, objective: &Objective, cancel: &Cancel) -> Result<Rep> {
        self.check(cancel).map_err(|error| GenerationFailure {
            message: error.to_string(),
            retryable: false,
        })?;
        let dir = tempfile::tempdir()?;
        let schema = dir.path().join("rep.schema.json");
        let output = dir.path().join("rep.json");
        std::fs::write(&schema, serde_json::to_vec(&generation_schema())?)?;
        let prompt = format!(
            "{}\nPROFILE:\n{}\nOBJECTIVE:\n{}",
            include_str!("../assets/generation.txt"),
            serde_json::to_string(profile)?,
            serde_json::to_string(objective)?
        );
        let mut cmd = command();
        cmd.current_dir(dir.path())
            .args([
                "exec",
                "--ignore-user-config",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "--ephemeral",
                "--json",
                "--output-schema",
            ])
            .arg(&schema)
            .arg("--output-last-message")
            .arg(&output)
            .arg("-");
        let result = process::run(&mut cmd, Some(&prompt), Duration::from_secs(180), cancel)?;
        if result.code != Some(0) {
            return Err(generation_failure(&result));
        }
        ensure!(
            std::fs::metadata(&output)
                .context("Codex produced no exercise")?
                .len()
                <= 64_000,
            "Generated rep exceeds size limit"
        );
        let rep: Rep = serde_json::from_slice(&std::fs::read(&output)?)
            .context("Codex returned an invalid exercise package")?;
        rep.validate()?;
        ensure!(
            rep.version == 2,
            "Generated rep must include structured examples (format v2)"
        );
        ensure!(
            rep.language == profile.language
                && rep.skill == objective.skill
                && rep.mode == objective.mode
                && rep.minutes <= profile.minutes,
            "Generated rep does not match the scheduled objective"
        );
        Ok(rep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_environment_only_contains_login_and_system_allowlist() {
        let cmd = command();
        let names: Vec<_> = cmd
            .get_envs()
            .filter_map(|(name, value)| value.map(|_| name.to_string_lossy().to_string()))
            .collect();
        assert!(names.iter().all(|key| {
            [
                "PATH",
                "HOME",
                "USER",
                "CODEX_HOME",
                "TMPDIR",
                "TEMP",
                "TMP",
                "SYSTEMROOT",
                "NO_COLOR",
            ]
            .contains(&key.as_str())
        }));
        assert!(!names.contains(&"OPENAI_API_KEY".into()));
        assert!(!names.contains(&"CODEX_API_KEY".into()));
    }
}
