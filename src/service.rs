use crate::{
    model::{Attempt, Language, Mode, Profile, Rep},
    process::{self, Cancel},
    provider::{Codex, Provider},
    runner::{self, ContainerEngine},
    scheduler,
    store::Store,
};
use anyhow::{Result, bail, ensure};

pub fn bundled(language: Language, mode: Mode) -> Rep {
    let source = match (language, mode) {
        (Language::Python, Mode::Build) => include_str!("../assets/python-build.json"),
        (Language::Python, Mode::Debug) => include_str!("../assets/python-debug.json"),
        (Language::Python, Mode::Test) => include_str!("../assets/python-test.json"),
        (Language::Typescript, Mode::Build) => include_str!("../assets/typescript-build.json"),
        (Language::Typescript, Mode::Debug) => include_str!("../assets/typescript-debug.json"),
        (Language::Typescript, Mode::Test) => include_str!("../assets/typescript-test.json"),
    };
    serde_json::from_str(source).expect("embedded exercise is valid JSON")
}

pub fn prepare(
    store: &Store,
    profile: &Profile,
    count: usize,
    offline: bool,
    cancel: &Cancel,
    status: impl Fn(&str),
) -> Result<usize> {
    ensure!((1..=5).contains(&count), "Prepare 1–5 reps at a time");
    ensure!(
        store.queue(&profile.name)?.len() + count <= 5,
        "Queue has a five-rep limit. Practise a cached rep first."
    );
    let engine = ContainerEngine::configured()?;
    engine.available(profile.language, cancel)?;
    // Snapshot the requested settings for this batch, including background TUI jobs.
    let codex = Codex {
        settings: if offline {
            Default::default()
        } else {
            store.generation_settings()?
        },
    };
    let generation_status = format!(
        "Generating exercise · {} · uses your Codex allowance",
        codex.settings.summary()
    );
    let provenance = if offline {
        "bundled-v2".to_owned()
    } else {
        format!(
            "codex-cli; {}; requested {}",
            crate::generation::CONTRACT_VERSION,
            codex.settings.summary()
        )
    };
    let mut history = store.observations(&profile.name)?;
    let mut recent = store.recent_reps(&profile.name)?;
    let mut admitted = 0;
    for i in 0..count {
        process::cancelled(cancel)?;
        let mut objective = scheduler::select(profile, &history);
        scheduler::vary(&mut objective, profile, &recent);
        let mut last = String::new();
        for _ in 0..if offline { 1 } else { 2 } {
            status(if offline {
                "Checking bundled exercise"
            } else {
                &generation_status
            });
            let candidate = if offline {
                Ok(bundled(
                    profile.language,
                    [Mode::Debug, Mode::Build, Mode::Test][i % 3],
                ))
            } else {
                codex.generate(
                    profile,
                    &objective,
                    (!last.is_empty()).then_some(last.as_str()),
                    cancel,
                )
            };
            match candidate.and_then(|rep| {
                status("Checking reference, starter, and negative implementations");
                runner::validate(&rep, &engine, cancel)?;
                Ok(rep)
            }) {
                Ok(rep) => {
                    process::cancelled(cancel)?;
                    store.admit(
                        &profile.name,
                        &rep,
                        &format!(
                            "{}; schema-v{}; {} validated",
                            provenance,
                            rep.version,
                            engine.name()
                        ),
                    )?;
                    recent.insert(
                        0,
                        scheduler::RecentRep {
                            family: rep.family.clone(),
                            mode: rep.mode,
                            design: rep.design,
                        },
                    );
                    history.insert(
                        0,
                        crate::model::Observation {
                            skill: rep.skill,
                            family: rep.family,
                            mode: rep.mode,
                            outcome: "passed".into(),
                            assisted: false,
                        },
                    );
                    admitted += 1;
                    break;
                }
                Err(error) => {
                    last = error.to_string();
                    process::cancelled(cancel)?;
                    if !crate::provider::retryable(&error) {
                        break;
                    }
                }
            }
        }
        if admitted <= i {
            bail!("Prepared {admitted} rep(s). Could not prepare another: {last}");
        }
    }
    Ok(admitted)
}

pub fn next_cached(store: &mut Store, profile: &Profile) -> Result<Option<(Rep, Attempt)>> {
    if let Some(mut attempt) = store
        .attempts(&profile.name)?
        .into_iter()
        .find(|a| a.outcome == "in-progress")
    {
        let rep = store.rep(&attempt.rep_id)?;
        // Only move an untouched legacy example seed. Preserve all learner edits.
        if attempt.tests.trim() == rep.visible_tests.trim() {
            attempt.tests.clear();
            store.save_attempt(&attempt)?;
        }
        return Ok(Some((rep, attempt)));
    }
    let objective = scheduler::select(profile, &store.observations(&profile.name)?);
    let mut queue = store.queue(&profile.name)?;
    queue.sort_by_key(|(_, r)| {
        (
            r.skill != objective.skill,
            r.mode != objective.mode,
            objective.avoid_families.contains(&r.family),
        )
    });
    if let Some((id, rep)) = queue.into_iter().next() {
        let attempt = Attempt {
            id: uuid::Uuid::new_v4().to_string(),
            rep_id: id,
            profile: profile.name.clone(),
            code: rep.starter.clone(),
            tests: String::new(),
            hints: 0,
            viewed_solution: false,
            active_seconds: 0,
            outcome: "in-progress".into(),
        };
        store.save_attempt(&attempt)?;
        return Ok(Some((rep, attempt)));
    }
    Ok(None)
}

/// Start a fresh attempt on an already stored package, without generating or changing it.
pub fn revisit(store: &mut Store, profile: &Profile, id: Option<&str>) -> Result<()> {
    let attempts = store.attempts(&profile.name)?;
    ensure!(
        !attempts.iter().any(|a| a.outcome == "in-progress"),
        "Finish or replace your current rep before revisiting another"
    );
    let previous = attempts
        .iter()
        .find(|a| a.outcome == "passed" && id.is_none_or(|id| a.id == id))
        .ok_or_else(|| {
            anyhow::anyhow!("No matching completed attempt in this profile; see `spar history`")
        })?;
    let rep = store.rep(&previous.rep_id)?;
    let attempt = Attempt {
        id: uuid::Uuid::new_v4().to_string(),
        rep_id: previous.rep_id.clone(),
        profile: profile.name.clone(),
        code: rep.starter,
        tests: String::new(),
        hints: 0,
        viewed_solution: false,
        active_seconds: 0,
        outcome: "in-progress".into(),
    };
    store.save_attempt(&attempt)
}
