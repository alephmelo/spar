//! Strict provider response contract, separate from backward-compatible stored reps.
use crate::{
    model::{Check, Example, Language, Mode, Mutant, Profile, Rep, Requirement},
    scheduler::Objective,
};
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error};

pub const CONTRACT_VERSION: &str = "generation-v5";

fn prose(value: &str) -> bool {
    !value.chars().any(char::is_control)
        && !value.contains("```")
        && !["input:", "output:", "example:", "examples:"]
            .iter()
            .any(|marker| value.to_lowercase().contains(marker))
}

// The schema constrains model output; deserialization independently enforces the
// byte limits and semantic rules before any content reaches the runner. JSON
// Schema counts characters; local byte limits also bound non-ASCII payloads.
macro_rules! text_type {
    ($name:ident, $max:literal, $pattern:literal, $valid:expr) => {
        #[derive(Debug, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(
            #[schemars(length(min = 1, max = $max), regex(pattern = $pattern))] String,
        );

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                if value.trim().is_empty() || value.len() > $max || !($valid)(&value) {
                    return Err(D::Error::custom(concat!("Invalid ", stringify!($name))));
                }
                Ok(Self(value))
            }
        }
    };
}

text_type!(Title, 100, "^[^\\r\\n]+$", prose);
text_type!(Prose, 320, "^[^\\r\\n]+$", prose);
text_type!(Family, 80, "^[a-z0-9]+(-[a-z0-9]+)*$", |s: &str| {
    s.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
});
text_type!(Source, 8000, "[\\s\\S]+", |s: &str| s.lines().count()
    <= 100
    && !s.contains("```"));
text_type!(JsonText, 2000, "[\\s\\S]+", |s: &str| {
    serde_json::from_str::<serde_json::Value>(s).is_ok()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RequirementId {
    R1,
    R2,
    R3,
    R4,
    R5,
    R6,
}
impl RequirementId {
    fn label(self) -> String {
        format!("{self:?}")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum JsonKind {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}
impl JsonKind {
    fn accepts(self, value: &serde_json::Value) -> bool {
        match self {
            Self::Null => value.is_null(),
            Self::Boolean => value.is_boolean(),
            Self::Integer => value.is_i64() || value.is_u64(),
            Self::Number => value.is_number(),
            Self::String => value.is_string(),
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
        }
    }
    fn label(self) -> String {
        format!("{self:?}").to_lowercase()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValueSpec {
    pub kind: JsonKind,
    /// Describe fields or elements and their types; no example payloads.
    pub description: Prose,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    /// One short scenario paragraph. The app supplies the mode-specific task.
    pub summary: Prose,
    pub input: ValueSpec,
    pub output: ValueSpec,
    /// Guaranteed input bounds/types and units. Rejection rules belong in requirements.
    #[schemars(length(min = 1, max = 4))]
    pub constraints: Vec<Prose>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedRequirement {
    pub id: RequirementId,
    pub description: Prose,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedCheck {
    pub requirement: RequirementId,
    /// Short learner-visible name for the behavior checked, without solution hints.
    pub name: Title,
    /// Executable assertion statements; solve is supplied by the runner.
    pub code: Source,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedMutant {
    pub requirement: RequirementId,
    /// A complete incorrect implementation of solve, not an assertion snippet.
    pub code: Source,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedExample {
    /// A string containing valid JSON, checked locally against the input kind.
    pub input_json: JsonText,
    /// A string containing valid JSON, checked locally against the output kind.
    pub output_json: JsonText,
    pub explanation: Prose,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hints {
    /// Direct attention to a concept without naming the edit or solution.
    pub concept: Prose,
    /// Suggest a reasoning or testing strategy without giving the implementation.
    pub strategy: Prose,
    /// Final hint may reveal the concrete fix or test.
    pub solution: Prose,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedRep {
    /// Select exactly one ID from objective.topic_candidates; do not combine topics.
    pub topic: crate::topics::TopicId,
    pub title: Title,
    pub family: Family,
    pub brief: Brief,
    #[schemars(length(min = 1, max = 6))]
    pub requirements: Vec<GeneratedRequirement>,
    pub starter: Source,
    #[schemars(length(min = 1, max = 3))]
    pub examples: Vec<GeneratedExample>,
    #[schemars(length(min = 1, max = 10))]
    pub checks: Vec<GeneratedCheck>,
    pub hints: Hints,
    pub reference: Source,
    pub reference_tests: Source,
    #[schemars(length(min = 1, max = 3))]
    pub mutants: Vec<GeneratedMutant>,
    pub explanation: Prose,
}

impl GeneratedRep {
    pub fn into_rep(self, profile: &Profile, objective: &Objective) -> Result<Rep> {
        ensure!(
            objective
                .topic_candidates
                .iter()
                .any(|candidate| candidate.id == self.topic),
            "Exercise topic is not in the requested shortlist"
        );
        ensure!(
            (1..=4).contains(&self.brief.constraints.len()),
            "Expected 1–4 constraints"
        );
        ensure!(
            (1..=3).contains(&self.examples.len()),
            "Expected 1–3 examples"
        );
        ensure!(
            !objective.avoid_families.contains(&self.family.0),
            "Exercise repeats a recent family"
        );
        ensure!(
            self.requirements
                .iter()
                .enumerate()
                .all(|(index, requirement)| requirement.id as usize == index),
            "Requirement IDs must be consecutive from R1"
        );
        let reading_words = [
            &self.brief.summary,
            &self.brief.input.description,
            &self.brief.output.description,
        ]
        .into_iter()
        .chain(self.brief.constraints.iter())
        .chain(self.requirements.iter().map(|r| &r.description))
        .chain(self.examples.iter().map(|e| &e.explanation))
        .map(|text| text.0.split_whitespace().count())
        .sum::<usize>()
            + self
                .checks
                .iter()
                .map(|c| c.name.0.split_whitespace().count())
                .sum::<usize>();
        ensure!(
            reading_words
                <= if objective.compact_presentation {
                    220
                } else {
                    360
                },
            "Learner-facing prose exceeds the reading budget; shorten it without removing the contract"
        );
        let mut examples = Vec::new();
        for example in self.examples {
            let input: serde_json::Value = serde_json::from_str(&example.input_json.0)?;
            let output: serde_json::Value = serde_json::from_str(&example.output_json.0)?;
            ensure!(
                self.brief.input.kind.accepts(&input),
                "Example input does not match the declared JSON kind"
            );
            ensure!(
                self.brief.output.kind.accepts(&output),
                "Example output does not match the declared JSON kind"
            );
            let input_json = input.to_string();
            ensure!(
                !examples
                    .iter()
                    .any(|existing: &Example| existing.input_json == input_json),
                "Duplicate example input"
            );
            examples.push(Example {
                input_json,
                output_json: output.to_string(),
                explanation: example.explanation.0,
            });
        }
        let task = match objective.mode {
            Mode::Build => "Implement solve(value) from the requirements below.",
            Mode::Debug => "Fix the implementation and add a regression test that catches its bug.",
            Mode::Test => {
                "Write tests that distinguish correct behavior from plausible bugs. The implementation is read-only."
            }
        };
        let interface = match profile.language {
            Language::Python => "def solve(value)",
            Language::Typescript => "export function solve(value)",
        };
        let brief = format!(
            "{}\n\nYOUR TASK\n{task}\n\nINTERFACE\n{interface}\n\nINPUT\n{}: {}\n\nOUTPUT\n{}: {}\n\nINPUT GUARANTEES\n{}",
            self.brief.summary.0,
            self.brief.input.kind.label(),
            self.brief.input.description.0,
            self.brief.output.kind.label(),
            self.brief.output.description.0,
            self.brief
                .constraints
                .iter()
                .map(|c| format!("• {}", c.0))
                .collect::<Vec<_>>()
                .join("\n")
        );
        // Examples have one source of truth. Generate their executable assertions
        // in the app instead of requesting a redundant visible_tests field.
        let visible_tests = examples[0].assertion(profile.language);
        let rep = Rep {
            version: 2,
            title: self.title.0,
            language: profile.language,
            mode: objective.mode,
            skill: objective.skill.clone(),
            family: self.family.0,
            minutes: profile.minutes,
            brief,
            requirements: self
                .requirements
                .into_iter()
                .map(|r| Requirement {
                    id: r.id.label(),
                    description: r.description.0,
                })
                .collect(),
            starter: self.starter.0,
            visible_tests,
            examples,
            checks: self
                .checks
                .into_iter()
                .map(|c| Check {
                    requirement: c.requirement.label(),
                    name: Some(c.name.0),
                    code: c.code.0,
                })
                .collect(),
            hints: vec![
                self.hints.concept.0,
                self.hints.strategy.0,
                self.hints.solution.0,
            ],
            reference: self.reference.0,
            reference_tests: self.reference_tests.0,
            mutants: self
                .mutants
                .into_iter()
                .map(|m| Mutant {
                    requirement: m.requirement.label(),
                    code: m.code.0,
                })
                .collect(),
            explanation: self.explanation.0,
            design: None,
            topic: Some(self.topic),
        };
        rep.validate()?;
        Ok(rep)
    }
}
