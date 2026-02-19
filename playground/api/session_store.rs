use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

struct SessionRecord {
    directory: PathBuf,
    last_accessed: SystemTime,
}

pub struct SessionStore {
    base_directory: PathBuf,
    sessions: Mutex<HashMap<String, SessionRecord>>,
    session_counter: AtomicU64,
}

impl SessionStore {
    #[must_use]
    pub fn new(base_directory: PathBuf) -> Self {
        fs::create_dir_all(&base_directory).unwrap_or_else(|error| {
            panic!(
                "failed to create playground session root {}: {error}",
                base_directory.display()
            )
        });
        Self {
            base_directory,
            sessions: Mutex::new(HashMap::new()),
            session_counter: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn create_session(&self) -> String {
        let session_id = self.generate_session_id();
        let session_directory = self.base_directory.join(&session_id);
        fs::create_dir_all(&session_directory).unwrap_or_else(|error| {
            panic!(
                "failed to create playground session dir {}: {error}",
                session_directory.display()
            )
        });

        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions.insert(
            session_id.clone(),
            SessionRecord {
                directory: session_directory,
                last_accessed: SystemTime::now(),
            },
        );
        session_id
    }

    pub fn session_directory(&self, session_id: &str) -> Option<PathBuf> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let record = sessions.get_mut(session_id)?;
        record.last_accessed = SystemTime::now();
        Some(record.directory.clone())
    }

    pub fn cleanup_expired(&self, max_idle: Duration) {
        let now = SystemTime::now();
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stale_ids = Vec::new();
        for (session_id, record) in sessions.iter() {
            let elapsed = now
                .duration_since(record.last_accessed)
                .unwrap_or_else(|_| Duration::from_secs(0));
            if elapsed > max_idle {
                stale_ids.push(session_id.clone());
            }
        }

        for session_id in stale_ids {
            if let Some(record) = sessions.remove(&session_id) {
                let _ = fs::remove_dir_all(record.directory);
            }
        }
    }

    fn generate_session_id(&self) -> String {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0));
        let counter = self.session_counter.fetch_add(1, Ordering::Relaxed);
        format!("{:x}{:x}", now.as_nanos(), counter)
    }
}

pub fn ensure_workspace_manifest(session_directory: &Path) -> std::io::Result<()> {
    let manifest_path = session_directory.join("PACKAGE.copp");
    if manifest_path.is_file() {
        return Ok(());
    }
    fs::write(manifest_path, "")
}
