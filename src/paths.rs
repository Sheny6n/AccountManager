use std::path::{Path, PathBuf};

pub fn salt_path_for(db: &Path) -> PathBuf {
    db.with_extension("salt")
}
