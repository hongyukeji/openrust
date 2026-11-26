use std::{
    fs,
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use jiff::Timestamp;
use openrust::log::{Level, LogContext};
use serde_json::json;

/// One active Pail capture session.
#[derive(Clone, Debug)]
pub struct File(PathBuf);

impl File {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn exists(&self) -> bool {
        self.0.exists()
    }

    pub fn create(&self) -> io::Result<()> {
        if let Some(parent) = self
            .0
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let new_directory = !parent.exists();
            fs::create_dir_all(parent)?;
            if new_directory {
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(parent.join(".gitignore"))
                {
                    Ok(mut file) => file.write_all(b"*\n!.gitignore\n")?,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
            }
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.0)
        {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn destroy(&self) -> io::Result<()> {
        match fs::remove_file(&self.0) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    pub fn log(&self, level: Level, message: &str, context: &LogContext) -> io::Result<()> {
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .append(!cfg!(windows))
            .open(&self.0)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        file.lock()?;
        let metadata = file.metadata()?;
        if SystemTime::now()
            .duration_since(metadata.modified()?)
            .is_ok_and(|age| age > Duration::from_secs(3600))
        {
            drop(file);
            return self.destroy();
        }
        file.seek(SeekFrom::End(0))?;
        let (number, name) = match level {
            Level::Debug => (100, "DEBUG"),
            Level::Info => (200, "INFO"),
            Level::Notice => (250, "NOTICE"),
            Level::Warning => (300, "WARNING"),
            Level::Error => (400, "ERROR"),
            Level::Critical => (500, "CRITICAL"),
            Level::Alert => (550, "ALERT"),
            Level::Emergency => (600, "EMERGENCY"),
        };
        let record = json!({"message":message,"context":context,"level":number,"level_name":name,
            "channel":"pail","datetime":Timestamp::now().strftime("%Y-%m-%dT%H:%M:%S%.6f%:z").to_string(),"extra":{}});
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        if let Err(error) = file.write_all(&line) {
            let _ = file.set_len(metadata.len());
            return Err(error);
        }
        Ok(())
    }
}

impl std::fmt::Display for File {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(formatter)
    }
}
