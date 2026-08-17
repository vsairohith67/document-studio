use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::path_policy::{canonical_directory, reject_reparse_components, PathPolicyError};

const ROOT_MARKER: &str = ".document-studio-workspaces-v1";
const JOB_MARKER: &str = ".document-studio-job-v1";

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace path policy failed")]
    Path(#[from] PathPolicyError),
    #[error("workspace filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("workspace identifier is not a UUID")]
    InvalidJobId,
    #[error("workspace ownership marker is missing or invalid")]
    OwnershipMarker,
    #[error("workspace escaped the application root")]
    EscapedRoot,
}

#[derive(Debug, Clone)]
pub struct JobWorkspace {
    pub root: PathBuf,
    pub inputs: PathBuf,
    pub staging: PathBuf,
    pub temporary: PathBuf,
    pub audit: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    root: PathBuf,
}

impl WorkspaceManager {
    pub fn initialize(app_data_directory: &Path) -> Result<Self, WorkspaceError> {
        fs::create_dir_all(app_data_directory)?;
        let app_data_directory = canonical_directory(app_data_directory)?;
        let root = app_data_directory.join("workspaces");
        if !root.exists() {
            fs::create_dir(&root)?;
        }
        reject_reparse_components(&root)?;
        create_marker(&root.join(ROOT_MARKER), "Document Studio workspace root v1")?;
        Ok(Self {
            root: canonical_directory(&root)?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create_job(&self, job_id: &str) -> Result<JobWorkspace, WorkspaceError> {
        let job_id = Uuid::parse_str(job_id).map_err(|_| WorkspaceError::InvalidJobId)?;
        let root = self.root.join(job_id.hyphenated().to_string());
        fs::create_dir(&root)?;
        fs::create_dir(root.join("inputs"))?;
        fs::create_dir(root.join("staging"))?;
        fs::create_dir(root.join("temp"))?;
        fs::create_dir(root.join("audit"))?;
        create_marker(&root.join(JOB_MARKER), &job_id.hyphenated().to_string())?;
        reject_reparse_components(&root)?;
        Ok(JobWorkspace {
            inputs: root.join("inputs"),
            staging: root.join("staging"),
            temporary: root.join("temp"),
            audit: root.join("audit"),
            root,
        })
    }

    pub fn cleanup_job(&self, job_id: &str) -> Result<(), WorkspaceError> {
        let job_id = Uuid::parse_str(job_id).map_err(|_| WorkspaceError::InvalidJobId)?;
        let expected_name = job_id.hyphenated().to_string();
        let root = self.root.join(&expected_name);
        if !root.exists() {
            return Ok(());
        }
        reject_reparse_components(&root)?;
        let canonical = canonical_directory(&root)?;
        if canonical.parent() != Some(self.root.as_path())
            || canonical.file_name() != Some(expected_name.as_ref())
        {
            return Err(WorkspaceError::EscapedRoot);
        }
        let mut marker = String::new();
        OpenOptions::new()
            .read(true)
            .open(canonical.join(JOB_MARKER))?
            .take(128)
            .read_to_string(&mut marker)?;
        if marker != expected_name {
            return Err(WorkspaceError::OwnershipMarker);
        }
        fs::remove_dir_all(canonical)?;
        Ok(())
    }
}

fn create_marker(path: &Path, expected: &str) -> Result<(), WorkspaceError> {
    if path.exists() {
        let mut contents = String::new();
        OpenOptions::new()
            .read(true)
            .open(path)?
            .take(128)
            .read_to_string(&mut contents)?;
        if contents != expected {
            return Err(WorkspaceError::OwnershipMarker);
        }
        return Ok(());
    }
    let mut marker = OpenOptions::new().write(true).create_new(true).open(path)?;
    marker.write_all(expected.as_bytes())?;
    marker.sync_all()?;
    Ok(())
}
