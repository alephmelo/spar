use crate::{
    config::GenerationSettings,
    generation::GeneratedRep,
    model::{Profile, Rep},
    process::{self, Cancel},
    scheduler::Objective,
};
use anyhow::{Context, Result, ensure};
use std::{process::Command, time::Duration};

pub trait Provider: Send + Sync {
    fn check(&self, cancel: &Cancel) -> Result<String>;
    fn generate(
        &self,
        profile: &Profile,
        objective: &Objective,
        feedback: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Rep>;
}

#[derive(Default)]
pub struct Codex {
    pub settings: GenerationSettings,
}

/// Generation has a dedicated strict contract; cached v1/v2 reps keep their reader.
pub fn generation_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::default()
        .for_serialize()
        // Codex rejects annotations beside $ref (e.g. a field's description).
        // This contract is nonrecursive, so inline types preserve all bounds
        // and descriptions without ref siblings or unsupported allOf wrappers.
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<GeneratedRep>()
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
            "The requested Codex model is unavailable for this account. Check `spar config` and choose an available model, or update the Codex CLI.",
            false,
        )
    } else if has(&[
        "reasoning_effort",
        "reasoning effort",
        "model_reasoning_effort",
    ]) {
        (
            "Codex rejected the reasoning effort. Choose a level supported by your model with `spar config --effort LEVEL`, or use `--effort default`.",
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
            "Codex rejected the generation request (invalid_request_error). Check `spar config` and update Spar and the Codex CLI.",
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
        self.settings.validate()?;
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
            "--model",
            "--config",
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
    fn generate(
        &self,
        profile: &Profile,
        objective: &Objective,
        feedback: Option<&str>,
        cancel: &Cancel,
    ) -> Result<Rep> {
        self.check(cancel).map_err(|error| GenerationFailure {
            message: error.to_string(),
            retryable: false,
        })?;
        let dir = tempfile::tempdir()?;
        let schema = dir.path().join("rep.schema.json");
        let output = dir.path().join("rep.json");
        std::fs::write(&schema, serde_json::to_vec(&generation_schema())?)?;
        let prompt = format!(
            "{}\nREQUEST DATA:\n{}",
            include_str!("../assets/generation.txt"),
            serde_json::to_string(&serde_json::json!({
                "profile": profile,
                "objective": objective,
                "previous_validation_failure": feedback,
            }))?
        );
        let mut cmd = command();
        // Pass values as distinct arguments, never through a shell or the prompt.
        // These explicit overrides work while unrelated user config stays disabled.
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
            .arg(&output);
        if let Some(model) = &self.settings.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(effort) = &self.settings.effort {
            cmd.arg("--config")
                .arg(format!("model_reasoning_effort=\"{effort}\""));
        }
        cmd.arg("-");
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
        let generated: GeneratedRep =
            serde_json::from_slice(&std::fs::read(&output)?).map_err(|error| {
                // Serde errors can quote response values, including hidden solutions.
                // Report only our known validation categories and the location.
                let message = error.to_string();
                let category = [
                    "Invalid Title",
                    "Invalid Prose",
                    "Invalid Family",
                    "Invalid Source",
                    "Invalid JsonText",
                ]
                .into_iter()
                .find(|category| message.starts_with(category))
                .unwrap_or("Missing, unknown, or incorrectly typed field");
                anyhow::anyhow!(
                    "Invalid generation contract: {category} at line {}, column {}",
                    error.line(),
                    error.column()
                )
            })?;
        generated.into_rep(profile, objective)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_schema_requires_every_field_and_nonempty_examples() {
        fn check_objects(value: &serde_json::Value) {
            match value {
                serde_json::Value::Object(object) => {
                    if object.get("type").and_then(|v| v.as_str()) == Some("object") {
                        assert_eq!(object["additionalProperties"], false);
                        let required = object["required"].as_array().unwrap();
                        for key in object["properties"].as_object().unwrap().keys() {
                            assert!(
                                required.iter().any(|value| value == key),
                                "missing required {key}"
                            );
                        }
                    }
                    for value in object.values() {
                        check_objects(value);
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        check_objects(value);
                    }
                }
                _ => {}
            }
        }
        let schema = serde_json::to_value(generation_schema()).unwrap();
        check_objects(&schema);
        assert_eq!(schema["properties"]["examples"]["minItems"], 1);
        assert_eq!(schema["properties"]["examples"]["maxItems"], 3);
        assert!(schema["properties"]["examples"].get("default").is_none());
    }

    #[test]
    fn rejected_schema_is_actionable_and_not_retried_or_leaked() {
        let failure = serde_json::json!({"type":"turn.failed", "error":{"message": "invalid_json_schema: Missing 'examples'."}});
        let output = process::Output {
            code: Some(1),
            stdout: failure.to_string(),
            stderr: String::new(),
            text: "unrevealed reference implementation".into(),
        };
        let error = generation_failure(&output);
        assert!(error.to_string().contains("invalid_json_schema"));
        assert!(!error.to_string().contains("codex login"));
        assert!(!error.to_string().contains("unrevealed"));
        assert!(!retryable(&error));
    }

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
