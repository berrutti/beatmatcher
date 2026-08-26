use std::path::{Path, PathBuf};

use crate::lock::LockIgnoringPoison;

pub(crate) const AUDIO_WAV: &str = "audio.wav";
pub(crate) const AUDIO_FLAC: &str = "audio.flac";
pub(crate) const SESSION_LOG: &str = "session.bms";
const MANIFEST: &str = "manifest.json";

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobKind {
    Recording,
    Render,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Manifest {
    pub kind: JobKind,
    pub started_at: u64,
    pub suggested_name: String,
    pub audio_file: Option<String>,
    pub log_file: Option<String>,
}

impl Recoverable {
    fn is_empty(&self) -> bool {
        self.audio_bytes == 0 && self.log_path.is_none()
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Recoverable {
    pub id: String,
    pub kind: JobKind,
    pub started_at: u64,
    pub suggested_name: String,
    pub audio_path: Option<String>,
    pub audio_bytes: u64,
    pub log_path: Option<String>,
}

/// One directory per unfinished piece of work. A directory that is still here when the
/// app starts is work the last run never delivered, which is the whole crash detector.
pub(crate) struct Recovery {
    root: std::sync::Mutex<Option<PathBuf>>,
    active_recordings: std::sync::Mutex<Vec<Job>>,
}

impl Recovery {
    pub(crate) fn new() -> Self {
        Self {
            root: std::sync::Mutex::new(None),
            active_recordings: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn set_root(&self, data_dir: &Path) {
        let root = data_dir.join("in-progress");
        if let Err(error) = std::fs::create_dir_all(&root) {
            log::warn!(
                "recovery disabled, cannot create {}: {error}",
                root.display()
            );
            return;
        }
        *self.root.locked() = Some(root);
    }

    /// Falls back to the temp directory rather than failing: losing the app data dir is
    /// no reason to record with no way back.
    fn root(&self) -> Result<PathBuf, String> {
        if let Some(root) = self.root.locked().clone() {
            return Ok(root);
        }
        let fallback = std::env::temp_dir().join("beatmatcher-in-progress");
        std::fs::create_dir_all(&fallback).map_err(|error| error.to_string())?;
        Ok(fallback)
    }

    pub(crate) fn set_active_recording(&self, job: Job) {
        self.active_recordings.locked().push(job);
    }

    /// Identified by the file it wrote, because a new recording can start while the
    /// previous one is still being saved.
    pub(crate) fn finish_recording(&self, recorded_file: &str) {
        let Some(dir) = Path::new(recorded_file).parent() else {
            return;
        };
        let mut jobs = self.active_recordings.locked();
        let Some(index) = jobs.iter().position(|job| job.dir == dir) else {
            return;
        };
        jobs.remove(index).finish();
    }

    pub(crate) fn begin(
        &self,
        kind: JobKind,
        suggested_name: &str,
        audio_file: Option<&str>,
        log_file: Option<&str>,
    ) -> Result<Job, String> {
        let started_at = unix_secs();
        let root = self.root()?;
        let dir = root.join(format!("{started_at}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let manifest = Manifest {
            kind,
            started_at,
            suggested_name: suggested_name.to_string(),
            audio_file: audio_file.map(str::to_string),
            log_file: log_file.map(str::to_string),
        };
        write_manifest(&dir, &manifest)?;
        Ok(Job { dir })
    }

    pub(crate) fn list(&self) -> Vec<Recoverable> {
        let Ok(root) = self.root() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut found: Vec<Recoverable> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                // A job that captured nothing before it died has nothing to offer back,
                // so it is swept rather than shown as a choice the user has to make.
                let job = read_job(&path).filter(|found| !found.is_empty());
                if job.is_none() {
                    std::fs::remove_dir_all(&path).ok();
                }
                job
            })
            .collect();
        found.sort_by_key(|item| item.started_at);
        found
    }

    /// Moves one file out to where the user asked for it. The move is what retires it:
    /// a job left holding nothing is swept by `list`, so saving needs no bookkeeping.
    pub(crate) fn save_file(&self, id: &str, file: &str, dest: &str) -> Result<(), String> {
        let dir = self.job_dir(id)?;
        let source = match file {
            "log" => Some(dir.join(SESSION_LOG)).filter(|path| path.is_file()),
            "audio" => [AUDIO_WAV, AUDIO_FLAC]
                .into_iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file()),
            other => return Err(format!("not a recoverable file: {other}")),
        }
        .ok_or_else(|| format!("nothing to recover: the {file} file is gone"))?;

        if std::fs::rename(&source, dest).is_err() {
            std::fs::copy(&source, dest).map_err(|error| error.to_string())?;
            // Left behind on failure rather than reported: the copy already reached the
            // user, and the job is simply offered again next time.
            if let Err(error) = std::fs::remove_file(&source) {
                log::warn!("recovery: {} survived its save: {error}", source.display());
            }
        }
        Ok(())
    }

    pub(crate) fn discard(&self, id: &str) -> Result<(), String> {
        let dir = self.job_dir(id)?;
        std::fs::remove_dir_all(&dir).map_err(|error| error.to_string())
    }

    pub(crate) fn job_dir(&self, id: &str) -> Result<PathBuf, String> {
        // Rejected rather than joined: an id is only ever one of our own directory
        // names, and a path separator in it would reach outside the recovery root.
        if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(format!("not a recovery id: {id}"));
        }
        let dir = self.root()?.join(id);
        if dir.is_dir() {
            Ok(dir)
        } else {
            Err(format!("no such recovery job: {id}"))
        }
    }
}

pub(crate) struct Job {
    pub(crate) dir: PathBuf,
}

impl Job {
    pub(crate) fn path(&self, file: &str) -> String {
        self.dir.join(file).to_string_lossy().into_owned()
    }

    /// The work reached the place the user asked for, so the job stops being recoverable.
    pub(crate) fn finish(&self) {
        if let Err(error) = std::fs::remove_dir_all(&self.dir) {
            log::warn!("recovery: cannot clear {}: {error}", self.dir.display());
        }
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let json = serde_json::to_string(manifest).map_err(|error| error.to_string())?;
    std::fs::write(dir.join(MANIFEST), json).map_err(|error| error.to_string())
}

fn read_job(dir: &Path) -> Option<Recoverable> {
    let json = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    let manifest: Manifest = serde_json::from_str(&json).ok()?;
    let id = dir.file_name()?.to_string_lossy().into_owned();
    let present = |file: &Option<String>| {
        let name = file.as_ref()?;
        let path = dir.join(name);
        path.is_file().then(|| path.to_string_lossy().into_owned())
    };
    let audio_path = present(&manifest.audio_file);
    let audio_bytes = audio_path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map_or(0, |meta| meta.len());
    Some(Recoverable {
        id,
        kind: manifest.kind,
        started_at: manifest.started_at,
        suggested_name: manifest.suggested_name,
        audio_path,
        audio_bytes,
        log_path: present(&manifest.log_file),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recovery_in(dir: &Path) -> Recovery {
        let recovery = Recovery::new();
        recovery.set_root(dir);
        recovery
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bm-recovery-test-{name}-{}", unix_secs()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_job_that_captured_something_is_offered_back() {
        let root = scratch("offered");
        let recovery = recovery_in(&root);
        let job = recovery
            .begin(
                JobKind::Recording,
                "set",
                Some(AUDIO_WAV),
                Some(SESSION_LOG),
            )
            .expect("a job");
        std::fs::write(job.path(AUDIO_WAV), [0u8; 64]).expect("audio");
        std::fs::write(job.path(SESSION_LOG), "{}").expect("log");

        let found = recovery.list();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].suggested_name, "set");
        assert_eq!(found[0].audio_bytes, 64);
        assert!(found[0].log_path.is_some());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_finished_job_is_no_longer_offered_back() {
        let root = scratch("finished");
        let recovery = recovery_in(&root);
        let job = recovery
            .begin(JobKind::Render, "mix", Some(AUDIO_WAV), None)
            .expect("a job");
        std::fs::write(job.path(AUDIO_WAV), [0u8; 8]).expect("audio");
        job.finish();
        assert!(recovery.list().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_job_that_captured_nothing_is_swept_rather_than_offered() {
        let root = scratch("empty");
        let recovery = recovery_in(&root);
        recovery
            .begin(JobKind::Render, "mix", Some(AUDIO_WAV), None)
            .expect("a job");
        assert!(recovery.list().is_empty());
        assert!(
            std::fs::read_dir(root.join("in-progress"))
                .expect("recovery root")
                .next()
                .is_none(),
            "the empty job directory is removed, not left to accumulate"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_id_that_is_not_one_of_ours_cannot_reach_outside_the_root() {
        let root = scratch("traversal");
        let recovery = recovery_in(&root);
        for id in ["", "..", "../..", "a/b", "a\\b"] {
            assert!(recovery.job_dir(id).is_err(), "{id} must be refused");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn saving_a_file_takes_it_out_of_the_job() {
        let root = scratch("save-audio");
        let recovery = recovery_in(&root);
        let job = recovery
            .begin(
                JobKind::Recording,
                "set",
                Some(AUDIO_WAV),
                Some(SESSION_LOG),
            )
            .expect("a job");
        std::fs::write(job.path(AUDIO_WAV), [7u8; 32]).expect("audio");
        std::fs::write(job.path(SESSION_LOG), "{}").expect("log");
        let id = recovery.list().first().expect("a job").id.clone();

        let dest = root.join("saved.wav");
        recovery
            .save_file(&id, "audio", &dest.to_string_lossy())
            .expect("saved");

        assert_eq!(std::fs::read(&dest).expect("saved audio").len(), 32);
        assert!(
            !PathBuf::from(job.path(AUDIO_WAV)).exists(),
            "the partial must not survive its own save"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_job_stays_recoverable_until_every_file_is_decided() {
        let root = scratch("both");
        let recovery = recovery_in(&root);
        let job = recovery
            .begin(
                JobKind::Recording,
                "set",
                Some(AUDIO_WAV),
                Some(SESSION_LOG),
            )
            .expect("a job");
        std::fs::write(job.path(AUDIO_WAV), [0u8; 32]).expect("audio");
        std::fs::write(job.path(SESSION_LOG), "{}").expect("log");
        let id = recovery.list().first().expect("a job").id.clone();

        recovery
            .save_file(&id, "audio", &root.join("saved.wav").to_string_lossy())
            .expect("audio saved");
        let still = recovery.list();
        assert_eq!(still.len(), 1, "the log has not been decided yet");
        assert_eq!(still[0].audio_bytes, 0);
        assert!(still[0].log_path.is_some());

        recovery
            .save_file(&id, "log", &root.join("saved.bms").to_string_lossy())
            .expect("log saved");
        assert!(
            recovery.list().is_empty(),
            "with both files decided the job is done"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_the_job_does_not_have_is_refused() {
        let root = scratch("missing");
        let recovery = recovery_in(&root);
        let job = recovery
            .begin(JobKind::Render, "mix", Some(AUDIO_WAV), None)
            .expect("a job");
        std::fs::write(job.path(AUDIO_WAV), [0u8; 4]).expect("audio");
        let id = recovery.list().first().expect("a job").id.clone();

        let dest = root.join("out.bms");
        assert!(recovery
            .save_file(&id, "log", &dest.to_string_lossy())
            .is_err());
        assert!(recovery
            .save_file(&id, "nonsense", &dest.to_string_lossy())
            .is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discarding_removes_the_whole_job() {
        let root = scratch("discard");
        let recovery = recovery_in(&root);
        let job = recovery
            .begin(
                JobKind::Recording,
                "set",
                Some(AUDIO_WAV),
                Some(SESSION_LOG),
            )
            .expect("a job");
        std::fs::write(job.path(AUDIO_WAV), [0u8; 16]).expect("audio");
        std::fs::write(job.path(SESSION_LOG), "{}").expect("log");

        let id = recovery.list().first().expect("a job").id.clone();
        recovery.discard(&id).expect("discarded");
        assert!(recovery.list().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
