use crate::model::{Mode, Observation, Profile};
use serde::Serialize;

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
        avoid_families: history
            .iter()
            .take(5)
            .map(|h| h.family.clone())
            .chain(std::iter::once(format!(
                "Fit interests: {}",
                profile.interests.join(", ")
            )))
            .collect(),
    }
}
