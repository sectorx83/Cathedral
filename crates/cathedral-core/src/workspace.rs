//! Safe management of Cathedral-owned experiment directories.

use std::path::{Component, Path, PathBuf};
use crate::{CathedralError, Result};

pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let canonical_data = data_dir.canonicalize()?;
        Ok(Self { root: canonical_data.join("experiments") })
    }

    pub fn initialize(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn create(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || !id.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            return Err(CathedralError::UnsafePath(PathBuf::from(id)));
        }
        self.initialize()?;
        let path = self.root.join(id);
        std::fs::create_dir(&path)?;
        Ok(path)
    }

    pub fn remove(&self, path: &Path) -> Result<()> {
        let root = self.root.canonicalize()?;
        if !path.is_absolute() {
            return Err(CathedralError::UnsafePath(path.to_path_buf()));
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CathedralError::UnsafePath(path.to_path_buf()));
        }
        let canonical = path.canonicalize()?;
        if canonical == root
            || !canonical.starts_with(&root)
            || canonical.parent() != Some(root.as_path())
            || canonical.components().any(|part| matches!(part, Component::ParentDir))
        {
            return Err(CathedralError::UnsafePath(canonical));
        }
        std::fs::remove_dir_all(canonical)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_remove_paths_outside_managed_root() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(temp.path()).unwrap();
        workspace.initialize().unwrap();
        assert!(matches!(
            workspace.remove(temp.path()),
            Err(CathedralError::UnsafePath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_workspaces() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(temp.path()).unwrap();
        workspace.initialize().unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = temp.path().join("experiments").join("linked");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            workspace.remove(&link),
            Err(CathedralError::UnsafePath(_))
        ));
        assert!(target.exists());
    }
}
