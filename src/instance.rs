use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};

/// The OS releases this descriptor's lock on normal exit or a crash.
pub struct Instance {
    _file: File,
}
impl Instance {
    pub fn acquire(path: &Path) -> Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Create application directory")?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .context("Open application lock")?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Self { _file: file }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(None);
        }
        Err(error).context("Lock application")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_instance_until_the_owner_exits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.lock");
        let owner = Instance::acquire(&path).unwrap().unwrap();
        assert!(Instance::acquire(&path).unwrap().is_none());
        drop(owner);
        assert!(Instance::acquire(&path).unwrap().is_some());
    }
}
