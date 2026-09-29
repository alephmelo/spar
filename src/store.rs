use crate::{
    config::GenerationSettings,
    model::{Attempt, Observation, Profile, Rep},
};
use anyhow::{Context, Result, ensure};
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

pub struct Store {
    pub root: PathBuf,
    db: Connection,
}

impl Store {
    pub fn open(root: Option<&Path>) -> Result<Self> {
        let root = root
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("SPAR_HOME").map(PathBuf::from))
            .or_else(|| ProjectDirs::from("dev", "spar", "spar").map(|p| p.data_local_dir().into()))
            .context("Cannot locate data directory; set SPAR_HOME")?;
        std::fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        }
        let db = Connection::open(root.join("spar.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS profiles (name TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS reps (id TEXT PRIMARY KEY, profile TEXT NOT NULL, json TEXT NOT NULL, provenance TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'ready', created INTEGER NOT NULL DEFAULT (unixepoch()));
            CREATE TABLE IF NOT EXISTS attempts (id TEXT PRIMARY KEY, rep_id TEXT NOT NULL REFERENCES reps(id), profile TEXT NOT NULL, json TEXT NOT NULL, updated INTEGER NOT NULL DEFAULT (unixepoch()));
            PRAGMA user_version=1;")?;
        Ok(Self {
            root: root.canonicalize()?,
            db,
        })
    }
    pub fn generation_settings(&self) -> Result<GenerationSettings> {
        let json: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='generation'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let settings: GenerationSettings = match json {
            Some(json) => {
                serde_json::from_str(&json).context("Invalid saved generation settings")?
            }
            None => GenerationSettings::default(),
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn save_generation_settings(&self, settings: &GenerationSettings) -> Result<()> {
        settings.validate()?;
        self.db.execute(
            "INSERT INTO settings (key,value) VALUES ('generation',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    pub fn save_profile(&self, p: &Profile) -> Result<()> {
        ensure!(
            !p.name.trim().is_empty() && p.name.len() <= 80,
            "Profile name must be 1–80 characters"
        );
        ensure!(
            (1..=15).contains(&p.minutes),
            "Session length must be 1–15 minutes"
        );
        ensure!(
            !p.experience.trim().is_empty() && p.experience.len() <= 300,
            "Invalid experience"
        );
        ensure!(
            !p.interests.is_empty()
                && p.interests.len() <= 8
                && p.interests.iter().all(|i| !i.is_empty() && i.len() <= 80),
            "Choose 1–8 short interests"
        );
        if let Some(existing) = self.profile(&p.name)? {
            ensure!(
                existing.language == p.language,
                "Use a new profile name for another language to preserve history"
            );
        }
        self.db.execute("INSERT INTO profiles VALUES (?1,?2) ON CONFLICT(name) DO UPDATE SET json=excluded.json", params![p.name, serde_json::to_string(p)?])?;
        self.activate(&p.name)
    }
    pub fn activate(&self, name: &str) -> Result<()> {
        ensure!(self.profile(name)?.is_some(), "Unknown profile: {name}");
        self.db.execute("INSERT INTO settings VALUES ('active',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [name])?;
        Ok(())
    }
    pub fn profile(&self, name: &str) -> Result<Option<Profile>> {
        self.db
            .query_row("SELECT json FROM profiles WHERE name=?1", [name], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|s| Ok(serde_json::from_str(&s)?))
            .transpose()
    }
    pub fn active(&self) -> Result<Option<Profile>> {
        let name = self
            .db
            .query_row("SELECT value FROM settings WHERE key='active'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        match name {
            Some(n) => self.profile(&n),
            None => Ok(None),
        }
    }
    pub fn profiles(&self) -> Result<Vec<Profile>> {
        self.db
            .prepare("SELECT json FROM profiles ORDER BY name")?
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?))
            .collect()
    }
    pub fn admit(&self, profile: &str, rep: &Rep, provenance: &str) -> Result<String> {
        rep.validate()?;
        let id = uuid::Uuid::new_v4().to_string();
        self.db.execute(
            "INSERT INTO reps (id,profile,json,provenance) VALUES (?1,?2,?3,?4)",
            params![id, profile, serde_json::to_string(rep)?, provenance],
        )?;
        Ok(id)
    }
    pub fn queue(&self, profile: &str) -> Result<Vec<(String, Rep)>> {
        self.db.prepare("SELECT id,json FROM reps WHERE profile=?1 AND state='ready' ORDER BY created,rowid")?.query_map([profile], |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?)))?
            .map(|r| { let (id,json)=r?; Ok((id,serde_json::from_str(&json)?)) }).collect()
    }
    pub fn rep(&self, id: &str) -> Result<Rep> {
        let json: String = self
            .db
            .query_row("SELECT json FROM reps WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&json)?)
    }
    pub fn save_attempt(&mut self, attempt: &Attempt) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO attempts (id,rep_id,profile,json) VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET json=excluded.json,updated=unixepoch()", params![attempt.id,attempt.rep_id,attempt.profile,serde_json::to_string(attempt)?])?;
        tx.execute(
            "UPDATE reps SET state=?1 WHERE id=?2",
            params![
                if attempt.outcome == "in-progress" {
                    "active"
                } else {
                    "done"
                },
                attempt.rep_id
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn attempts(&self, profile: &str) -> Result<Vec<Attempt>> {
        self.db.prepare("SELECT json FROM attempts WHERE profile=?1 ORDER BY updated DESC,rowid DESC LIMIT 100")?.query_map([profile], |r| r.get::<_, String>(0))?
            .map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn observations(&self, profile: &str) -> Result<Vec<Observation>> {
        self.attempts(profile)?
            .into_iter()
            .filter(|a| a.outcome != "in-progress")
            .map(|a| {
                let rep = self.rep(&a.rep_id)?;
                Ok(Observation {
                    skill: rep.skill,
                    family: rep.family,
                    mode: rep.mode,
                    outcome: a.outcome,
                    assisted: a.hints > 0 || a.viewed_solution,
                })
            })
            .collect()
    }
}
