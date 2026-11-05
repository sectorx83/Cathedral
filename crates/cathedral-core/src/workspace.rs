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
                match std::fs::create_dir(&self.root) {
                    Ok(()) => {}
                    Err(create_error)
                        if create_error.kind() == std::io::ErrorKind::AlreadyExists =>
                    {
                        let metadata = std::fs::symlink_metadata(&self.root)?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(CathedralError::UnsafePath(self.root.clone()));
                        }
                    }
                    Err(create_error) => return Err(create_error.into()),
                }
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
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
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

    /// Remove a workspace if it exists. This supports resuming a deletion
    /// after a crash between filesystem removal and registry cleanup.
    pub fn remove_if_exists(&self, path: &Path) -> Result<bool> {
        self.initialize()?;
        let root = self.root.canonicalize()?;
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(CathedralError::UnsafePath(path.to_path_buf()));
        }
        let parent = path
            .parent()
            .ok_or_else(|| CathedralError::UnsafePath(path.to_path_buf()))?
            .canonicalize()?;
        if parent != root || path.file_name().is_none() {
            return Err(CathedralError::UnsafePath(path.to_path_buf()));
        }

        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CathedralError::UnsafePath(path.to_path_buf()));
        }
        let canonical = path.canonicalize()?;
        if canonical.parent() != Some(root.as_path()) {
            return Err(CathedralError::UnsafePath(canonical));
        }
        std::fs::remove_dir_all(canonical)?;
        Ok(true)
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
    fn concurrent_initialization_is_idempotent() {
        use std::sync::{Arc, Barrier};

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let root = root.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let workspace = Workspace::new(&root).unwrap();
                    barrier.wait();
                    workspace.initialize()
                })
            })
            .collect();

        for worker in workers {
            worker.join().unwrap().unwrap();
        }
    }

    #[test]
    fn validates_only_direct_child_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(temp.path()).unwrap();
        let path = workspace
            .create("01234567-89ab-cdef-0123-456789abcdef")
            .unwrap();
        assert_eq!(
            workspace.validate(&path).unwrap(),
            path.canonicalize().unwrap()
        );

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
    #[test]
    fn remove_if_exists_rejects_parent_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(temp.path()).unwrap();
        workspace.initialize().unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let traversal = temp.path().join("experiments").join("..").join("outside");

        assert!(matches!(
            workspace.remove_if_exists(&traversal),
            Err(CathedralError::UnsafePath(_))
        ));
        assert!(outside.exists());
    }
}
