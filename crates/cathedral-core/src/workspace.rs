//! Safe management of Cathedral-owned experiment directories.

use crate::{CathedralError, Result};
use std::path::{Component, Path, PathBuf};

pub struct Workspace {
    data_root: PathBuf,
    root: PathBuf,
}

impl Workspace {
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let data_root = data_dir.canonicalize()?;
        Ok(Self {
            root: data_root.join("experiments"),
            data_root,
        })
    }

    pub fn initialize(&self) -> Result<()> {
        match std::fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(CathedralError::UnsafePath(self.root.clone()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&self.root)?;
            }
            Err(error) => return Err(error.into()),
        }

        let canonical_root = self.root.canonicalize()?;
        if canonical_root.parent() != Some(self.data_root.as_path()) {
            return Err(CathedralError::UnsafePath(canonical_root));
        }
        Ok(())
    }

    pub fn create(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            return Err(CathedralError::UnsafePath(PathBuf::from(id)));
        }
        self.initialize()?;
        let path = self.root.join(id);
        std::fs::create_dir(&path)?;
        Ok(path)
    }

    /// Validate that a recorded experiment path is a real directory directly
    /// inside Cathedral's managed workspace root.
    pub fn validate(&self, path: &Path) -> Result<PathBuf> {
        self.initialize()?;
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
            || canonical
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(CathedralError::UnsafePath(canonical));
        }
        Ok(canonical)
    }

    pub fn remove(&self, path: &Path) -> Result<()> {
        let canonical = self.validate(path)?;
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

    #[test]
    fn validates_only_direct_child_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(temp.path()).unwrap();
        let path = workspace.create("01234567-89ab-cdef-0123-456789abcdef").unwrap();
        assert_eq!(workspace.validate(&path).unwrap(), path.canonicalize().unwrap());

        let nested = path.join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert!(matches!(
            workspace.validate(&nested),
            Err(CathedralError::UnsafePath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_workspace_root() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&data).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, data.join("experiments")).unwrap();

        let workspace = Workspace::new(&data).unwrap();
        assert!(matches!(
            workspace.initialize(),
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
