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
    pub topic: Option<crate::topics::TopicId>,
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
    pub topic_candidates: Vec<crate::topics::Topic>,
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
        topic_candidates: Vec::new(),
    }
}

/// Keep learning objectives separate from the randomly shortlisted operations.
pub fn vary(objective: &mut Objective, profile: &Profile, recent: &[RecentRep]) {
    objective.topic_candidates =
        crate::topics::shortlist(&objective.skill, profile.minutes, recent);
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
