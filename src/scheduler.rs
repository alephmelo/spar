use crate::model::{Mode, Observation, Profile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExerciseShape {
    Chunking,
    Intervals,
    Thresholds,
    RetryLimits,
    Grouping,
    StableDeduplication,
    TopSelection,
    Normalization,
    Projection,
    Fallback,
    Validation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    Caching,
    Batching,
    Retries,
    EventStreams,
    JsonRecords,
    BackendValidation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepDesign {
    pub shape: ExerciseShape,
    pub scenario: Scenario,
}

/// Generated/queued/active packages count for freshness, regardless of performance.
#[derive(Debug, Clone)]
pub struct RecentRep {
    pub family: String,
    pub mode: Mode,
    pub design: Option<RepDesign>,
}

pub const SKILLS: &[&str] = &[
    "boundaries",
    "collections",
    "transformations",
    "error-handling",
    "testing",
];

#[derive(Debug, Clone, Serialize)]
pub struct Objective {
    pub skill: String,
    pub mode: Mode,
    pub variation: String,
    pub avoid_families: Vec<String>,
    pub recent_feedback: Vec<RepFeedback>,
    pub compact_presentation: bool,
    pub design: RepDesign,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackReason {
    TooLarge,
    Unclear,
    Broken,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepFeedback {
    pub family: String,
    pub reason: FeedbackReason,
}

/// History is newest first. Defect reports and replacements carry no skill signal.
pub fn select(profile: &Profile, history: &[Observation]) -> Objective {
    let practice: Vec<_> = history.iter().filter(|h| h.outcome == "passed").collect();
    let mut skill = SKILLS
        .iter()
        .min_by_key(|s| practice.iter().take(15).filter(|h| h.skill == **s).count())
        .unwrap()
        .to_string();
    if let Some(revisit) = practice.iter().take(5).find(|h| {
        h.assisted
            && !practice
                .iter()
                .take(2)
                .any(|r| r.skill == h.skill && !r.assisted)
    }) {
        skill = revisit.skill.clone();
    }
    let testing_due = practice.len() >= 3 && !practice.iter().take(4).any(|h| h.mode == Mode::Test);
    let mode = if testing_due || skill == "testing" {
        Mode::Test
    } else if practice.len() % 2 == 0 {
        Mode::Debug
    } else {
        Mode::Build
    };
    if testing_due {
        skill = "testing".into();
    }
    let independent = practice
        .iter()
        .filter(|h| h.skill == skill && !h.assisted)
        .count();
    Objective {
        skill,
        mode,
        variation: if independent >= 3 {
            "A slightly harder variation, still one small decision"
        } else {
            "One focused decision with explicit edge cases"
        }
        .into(),
        avoid_families: history.iter().take(5).map(|h| h.family.clone()).collect(),
        recent_feedback: history
            .iter()
            .take(5)
            .filter_map(|h| {
                let reason = match h.outcome.as_str() {
                    "too-large" => FeedbackReason::TooLarge,
                    "unclear" => FeedbackReason::Unclear,
                    "broken" => FeedbackReason::Broken,
                    _ => return None,
                };
                Some(RepFeedback {
                    family: h.family.clone(),
                    reason,
                })
            })
            .collect(),
        compact_presentation: profile.minutes <= 5
            || history.iter().take(3).any(|h| h.outcome == "too-large"),
        design: RepDesign {
            shape: ExerciseShape::Chunking,
            scenario: Scenario::Batching,
        },
    }
}

/// Penalize recent shapes, scenarios and pairs, choosing only shapes appropriate
/// to the scheduled skill. This changes presentation, never proficiency evidence.
pub fn vary(objective: &mut Objective, profile: &Profile, recent: &[RecentRep]) {
    use ExerciseShape::*;
    use Scenario::*;
    let shapes: &[ExerciseShape] = match objective.skill.as_str() {
        "boundaries" => &[Chunking, Intervals, RetryLimits, Thresholds],
        "collections" => &[Grouping, StableDeduplication, TopSelection, Chunking],
        "transformations" => &[Normalization, Projection, Grouping, StableDeduplication],
        "error-handling" => &[Fallback, Validation, RetryLimits],
        _ => &[
            Intervals,
            Grouping,
            StableDeduplication,
            Fallback,
            Projection,
        ],
    };
    let scenarios = [
        Batching,
        Caching,
        Retries,
        EventStreams,
        JsonRecords,
        BackendValidation,
    ];
    let interests = profile.interests.join(" ").to_lowercase();
    let mut candidates = Vec::new();
    for &shape in shapes {
        // Every skill has at least three shapes, so a two-rep cooldown leaves a
        // choice even when the learner deliberately revisits the same skill.
        if recent.iter().take(2).any(|rep| {
            rep.design
                .map(|d| d.shape)
                .or_else(|| legacy_design_hints(&rep.family).0)
                == Some(shape)
        }) {
            continue;
        }
        for scenario in scenarios {
            // Keep the combinations coherent; a retry-limit rep needs retries.
            if (shape == RetryLimits) != (scenario == Retries) {
                continue;
            }
            let interest = match scenario {
                Caching => "cach",
                Batching => "batch",
                Retries => "retr",
                EventStreams => "stream",
                JsonRecords => "json",
                BackendValidation => "backend",
            };
            let mut score: i32 = if interests.contains(interest) { -8 } else { 0 };
            for (index, previous) in recent.iter().take(12).enumerate() {
                let weight = (12 - index) as i32;
                let (past_shape, past_scenario) = previous
                    .design
                    .map(|d| (Some(d.shape), Some(d.scenario)))
                    .unwrap_or_else(|| legacy_design_hints(&previous.family));
                if past_shape == Some(shape) {
                    score += weight * 4;
                }
                if past_scenario == Some(scenario) {
                    score += weight;
                }
                if past_shape == Some(shape) && past_scenario == Some(scenario) {
                    score += weight * 3;
                }
            }
            candidates.push((score, RepDesign { shape, scenario }));
        }
    }
    objective.design = candidates
        .into_iter()
        .min_by_key(|(score, _)| *score)
        .expect("nonempty design catalog")
        .1;
    // A due test-writing objective keeps priority; other modes avoid long streaks.
    if objective.mode != Mode::Test
        && recent.len() >= 2
        && recent.iter().take(2).all(|rep| rep.mode == objective.mode)
    {
        objective.mode = if objective.mode == Mode::Debug {
            Mode::Build
        } else {
            Mode::Debug
        };
    }
    objective.avoid_families = recent.iter().take(12).map(|r| r.family.clone()).collect();
}

// Older packages have no design metadata. These are conservative scheduling
// hints from family names, not verified classifications or learner observations.
fn legacy_design_hints(family: &str) -> (Option<ExerciseShape>, Option<Scenario>) {
    let family = family.to_lowercase();
    let shape = if [
        "threshold",
        "limit",
        "freshness",
        "expiry",
        "expiration",
        "boundary",
    ]
    .iter()
    .any(|word| family.contains(word))
    {
        Some(ExerciseShape::Thresholds)
    } else {
        None
    };
    let scenario = if family.contains("cache") {
        Some(Scenario::Caching)
    } else if family.contains("batch") {
        Some(Scenario::Batching)
    } else if family.contains("record") {
        Some(Scenario::JsonRecords)
    } else {
        None
    };
    (shape, scenario)
}
