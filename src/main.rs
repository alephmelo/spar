use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use spar::{
    model::{Language, Profile},
    process,
    provider::{Codex, Provider},
    runner::ContainerEngine,
    service,
    store::Store,
    tui,
};
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(version, about = "Your agent sets the challenge. You write the code.")]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Override the local data directory (or set SPAR_HOME)"
    )]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Set up a practice profile; each language keeps its own history.
    Init {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum)]
        language: Option<Language>,
        #[arg(long)]
        experience: Option<String>,
        #[arg(long, value_delimiter = ',')]
        focus: Vec<String>,
        #[arg(long, value_parser=clap::value_parser!(u8).range(1..=15))]
        minutes: Option<u8>,
    },
    /// List profiles or switch the active profile.
    Profiles { name: Option<String> },
    /// Generate and validate a small offline queue (uses Codex allowance).
    Prepare {
        #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u8).range(1..=5))]
        count: u8,
        #[arg(long, help = "Validate built-in examples without an AI call")]
        offline: bool,
    },
    /// Start Apple container on macOS and prepare the Python/TypeScript runners.
    Setup,
    /// Check generation authentication and isolated runtimes.
    Doctor,
    /// Show observed practice; no proficiency scores.
    History,
    /// Revisit a completed rep with a fresh attempt, without an AI call.
    Revisit { attempt_id: Option<String> },
    /// Preview the editor with a bundled rep in a temporary profile; no model calls.
    Demo {
        #[arg(long, value_enum, default_value = "typescript")]
        language: Language,
    },
}

fn ask(label: &str, default: &str) -> Result<String> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        bail!("Onboarding requires input; use `spar init --help` for flags");
    }
    Ok(if line.trim().is_empty() {
        default.to_string()
    } else {
        line.trim().into()
    })
}
fn init(
    store: &Store,
    name: Option<String>,
    language: Option<Language>,
    experience: Option<String>,
    focus: Vec<String>,
    minutes: Option<u8>,
) -> Result<Profile> {
    println!("\nspar / make room for practice\n");
    let language = match language {
        Some(l) => l,
        None => match ask("Practice language (python/typescript)", "typescript")?
            .to_lowercase()
            .as_str()
        {
            "python" | "py" => Language::Python,
            "typescript" | "ts" => Language::Typescript,
            _ => bail!("Supported languages: python, typescript"),
        },
    };
    let name = match name {
        Some(n) => n,
        None => ask("Profile name", &language.to_string().to_lowercase())?,
    };
    let experience = match experience {
        Some(e) => e,
        None => ask("Experience", "Experienced, but rusty")?,
    };
    let interests = if focus.is_empty() {
        ask("Focus (comma separated)", "Backend,Data processing,Testing")?
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        focus
    };
    let minutes = match minutes {
        Some(m) => m,
        None => ask("Session length in minutes", "5")?
            .parse()
            .context("Session length must be 1–15")?,
    };
    let p = Profile {
        name,
        language,
        experience,
        interests,
        minutes,
    };
    store.save_profile(&p)?;
    println!(
        "\nProfile saved. Generation sends your selected profile and recent exercise families to Codex,\nusing your existing ChatGPT login and normal Codex allowance. Your code stays local.\n\nNext: `spar setup`, then `spar`. Try `spar demo` to preview the editor.\n"
    );
    Ok(p)
}
fn profile(store: &Store) -> Result<Profile> {
    store
        .active()?
        .context("Run `spar init` to choose your language and goals")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cancel = process::cancel_token();
    let signal_cancel = cancel.clone();
    ctrlc::set_handler(move || {
        signal_cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    })?;
    if let Some(Commands::Demo { language }) = cli.command {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(Some(dir.path()))?;
        let p = Profile {
            name: "Preview".into(),
            language,
            experience: "Rusty".into(),
            interests: vec!["Caching".into()],
            minutes: 5,
        };
        store.save_profile(&p)?;
        store.admit(
            &p.name,
            &service::bundled(language, spar::model::Mode::Debug),
            "preview only; not execution validated",
        )?;
        return tui::run(&mut store, p, true);
    }
    let mut store = Store::open(cli.data_dir.as_deref())?;
    match cli.command {
        Some(Commands::Init {
            name,
            language,
            experience,
            focus,
            minutes,
        }) => {
            init(&store, name, language, experience, focus, minutes)?;
        }
        Some(Commands::Profiles { name }) => {
            if let Some(n) = name {
                store.activate(&n)?;
            }
            let active = store.active()?.map(|p| p.name);
            for p in store.profiles()? {
                println!(
                    "{} {} · {} · {} · {} min",
                    if Some(&p.name) == active.as_ref() {
                        "●"
                    } else {
                        "○"
                    },
                    p.name,
                    p.language,
                    p.experience,
                    p.minutes
                );
            }
        }
        Some(Commands::Setup) => {
            ContainerEngine::configured()?.setup(&cancel)?;
            println!("Runners ready. Run `spar`.");
        }
        Some(Commands::Prepare { count, offline }) => {
            let n = service::prepare(
                &store,
                &profile(&store)?,
                count as usize,
                offline,
                &cancel,
                |s| println!("{s}"),
            )?;
            println!("{n} validated rep(s) ready.");
        }
        Some(Commands::Doctor) => {
            println!("Data: {}", store.root.display());
            println!("Runner: {}", ContainerEngine::configured()?.name());
            println!(
                "{}",
                Codex
                    .check(&cancel)
                    .unwrap_or_else(|e| format!("Codex: {e}"))
            );
            for l in [Language::Python, Language::Typescript] {
                println!(
                    "{l}: {}",
                    ContainerEngine::configured()?
                        .available(l, &cancel)
                        .map(|_| "ready".into())
                        .unwrap_or_else(|e| e.to_string())
                );
            }
        }
        Some(Commands::History) => {
            let p = profile(&store)?;
            let attempts = store.attempts(&p.name)?;
            if attempts.is_empty() {
                println!("No practice recorded yet.");
            }
            for a in attempts {
                let r = store.rep(&a.rep_id)?;
                println!(
                    "{} · {} · {} · {} · {}s\n  {}",
                    r.title,
                    r.skill,
                    a.outcome,
                    a.assistance(),
                    a.active_seconds,
                    a.id
                );
            }
        }
        Some(Commands::Revisit { attempt_id }) => {
            let p = profile(&store)?;
            service::revisit(&mut store, &p, attempt_id.as_deref())?;
            tui::run(&mut store, p, false)?;
        }
        None => {
            let p = match store.active()? {
                Some(p) => p,
                None => init(&store, None, None, None, vec![], None)?,
            };
            tui::run(&mut store, p, false)?;
        }
        Some(Commands::Demo { .. }) => unreachable!(),
    }
    Ok(())
}
