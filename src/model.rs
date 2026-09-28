use anyhow::{Result, ensure};
use clap::ValueEnum;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Typescript,
}

impl Language {
    pub fn file(self) -> &'static str {
        match self {
            Self::Python => "solution.py",
            Self::Typescript => "solution.ts",
        }
    }
    pub fn test_file(self) -> &'static str {
        match self {
            Self::Python => "tests.py",
            Self::Typescript => "tests.ts",
        }
    }
    pub fn image(self) -> &'static str {
        match self {
            Self::Python => "python:3.13-alpine",
            Self::Typescript => "node:24-alpine",
        }
    }
}
impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Python => "Python",
            Self::Typescript => "TypeScript",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Build,
    Debug,
    Test,
}
impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub language: Language,
    pub experience: String,
    pub interests: Vec<String>,
    pub minutes: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub requirement: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mutant {
    pub requirement: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Example {
    /// A JSON-encoded argument to solve(value).
    pub input_json: String,
    /// The JSON-encoded expected return value.
    pub output_json: String,
    pub explanation: String,
}

/// Code is content only. File names and all execution commands belong to adapters.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rep {
    pub version: u8,
    pub title: String,
    pub language: Language,
    pub mode: Mode,
    pub skill: String,
    pub family: String,
    pub minutes: u8,
    pub brief: String,
    pub requirements: Vec<Requirement>,
    pub starter: String,
    pub visible_tests: String,
    #[serde(default)]
    #[schemars(length(min = 1, max = 3))]
    pub examples: Vec<Example>,
    pub checks: Vec<Check>,
    pub hints: Vec<String>,
    pub reference: String,
    pub reference_tests: String,
    pub mutants: Vec<Mutant>,
    pub explanation: String,
}

impl Rep {
    pub fn validate(&self) -> Result<()> {
        ensure!((1..=2).contains(&self.version), "Unsupported rep format");
        ensure!(
            self.examples.len() <= 3 && (self.version == 1 || !self.examples.is_empty()),
            "Expected 1–3 examples for format v2"
        );
        for example in &self.examples {
            ensure!(
                example.input_json.len() <= 2000
                    && example.output_json.len() <= 2000
                    && example.explanation.len() <= 1000,
                "Example exceeds size limit"
            );
            serde_json::from_str::<serde_json::Value>(&example.input_json)?;
            serde_json::from_str::<serde_json::Value>(&example.output_json)?;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 64_000,
            "Rep exceeds 64 KB"
        );
        ensure!(
            !self.title.trim().is_empty() && self.title.len() <= 100,
            "Invalid title"
        );
        ensure!(
            (1..=15).contains(&self.minutes),
            "Rep duration must be 1–15 minutes"
        );
        ensure!(
            !self.brief.trim().is_empty() && !self.explanation.trim().is_empty(),
            "Missing teaching material"
        );
        ensure!(
            crate::scheduler::SKILLS.contains(&self.skill.as_str()),
            "Unsupported skill"
        );
        ensure!(
            !self.family.is_empty() && self.family.len() <= 80,
            "Invalid family"
        );
        ensure!(
            (1..=6).contains(&self.requirements.len()),
            "Expected 1–6 requirements"
        );
        ensure!(
            (1..=10).contains(&self.checks.len()),
            "Expected 1–10 checks"
        );
        ensure!(
            (1..=3).contains(&self.mutants.len()),
            "Expected 1–3 negative implementations"
        );
        ensure!(
            (2..=4).contains(&self.hints.len()),
            "Expected 2–4 progressive hints"
        );
        for source in [
            &self.starter,
            &self.reference,
            &self.visible_tests,
            &self.reference_tests,
        ] {
            ensure!(
                !source.trim().is_empty() && source.len() <= 8000 && source.lines().count() <= 100,
                "Source must be nonempty and at most 100 lines / 8 KB"
            );
        }
        let ids: std::collections::HashSet<_> = self.requirements.iter().map(|r| &r.id).collect();
        ensure!(
            ids.len() == self.requirements.len(),
            "Duplicate requirement IDs"
        );
        for req in &self.requirements {
            ensure!(
                !req.id.is_empty() && !req.description.is_empty(),
                "Empty requirement"
            );
            ensure!(
                self.checks.iter().any(|c| c.requirement == req.id),
                "Requirement has no check"
            );
        }
        for (id, code) in self
            .checks
            .iter()
            .map(|x| (&x.requirement, &x.code))
            .chain(self.mutants.iter().map(|x| (&x.requirement, &x.code)))
        {
            ensure!(
                ids.contains(id) && !code.trim().is_empty() && code.len() <= 8000,
                "Invalid check or mutant"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub id: String,
    pub rep_id: String,
    pub profile: String,
    pub code: String,
    pub tests: String,
    pub hints: usize,
    pub viewed_solution: bool,
    pub active_seconds: u64,
    pub outcome: String,
}
impl Attempt {
    pub fn assistance(&self) -> &'static str {
        if self.viewed_solution {
            "Viewed solution"
        } else if self.hints > 0 {
            "Used hints"
        } else {
            "Independent"
        }
    }
}

#[derive(Debug, Clone)]
pub struct Observation {
    pub skill: String,
    pub family: String,
    pub mode: Mode,
    pub outcome: String,
    pub assisted: bool,
}
