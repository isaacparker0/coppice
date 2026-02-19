use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileRole {
    Library,
    BinaryEntrypoint,
    Test,
    PackageManifest,
}

impl FileRole {
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let file_name = path.file_name()?.to_str()?;
        if file_name == "PACKAGE.copp" {
            return Some(Self::PackageManifest);
        }
        if file_name.ends_with(".bin.copp") {
            return Some(Self::BinaryEntrypoint);
        }
        if file_name.ends_with(".test.copp") {
            return Some(Self::Test);
        }
        if file_name.ends_with(".copp") {
            return Some(Self::Library);
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub id: FileId,
    pub workspace_relative_path: PathBuf,
    pub role: FileRole,
}
