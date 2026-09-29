use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Requested generation overrides. None delegates to Codex's defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GenerationSettings {
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl GenerationSettings {
    pub fn validate(&self) -> Result<()> {
        if let Some(model) = &self.model {
            ensure!(
                !model.is_empty()
                    && model.len() <= 128
                    && model.starts_with(|c: char| c.is_ascii_alphanumeric())
                    && model
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-._/:".contains(c)),
                "Model must be a model ID of 1–128 characters, such as the ID shown in Codex's model picker"
            );
        }
        if let Some(effort) = &self.effort {
            ensure!(
                [
                    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"
                ]
                .contains(&effort.as_str()),
                "Effort must be none, minimal, low, medium, high, xhigh, max, ultra, or default; support depends on the model and Codex version"
            );
        }
        Ok(())
    }

    pub fn model_label(&self) -> &str {
        self.model.as_deref().unwrap_or("Codex default")
    }

    pub fn effort_label(&self) -> &str {
        self.effort.as_deref().unwrap_or("model default")
    }

    pub fn summary(&self) -> String {
        format!(
            "model: {} · effort: {}",
            self.model_label(),
            self.effort_label()
        )
    }
}
