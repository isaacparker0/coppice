use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use compiler__source::SourceFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPackage {
    pub id: PackageId,
    pub package_path: String,
    pub root_directory: PathBuf,
    pub manifest_path: PathBuf,
    pub source_files: Vec<SourceFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    root_directory: PathBuf,
    packages: Vec<DiscoveredPackage>,
    package_id_by_path: BTreeMap<String, PackageId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryError {
    pub message: String,
    pub path: Option<PathBuf>,
}

impl DiscoveryError {
    #[must_use]
    pub fn new(message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            message: message.into(),
            path,
        }
    }
}

impl Workspace {
    pub(crate) fn new(
        root_directory: PathBuf,
        packages: Vec<DiscoveredPackage>,
    ) -> Result<Self, DiscoveryError> {
        let mut package_id_by_path = BTreeMap::new();
        for package in &packages {
            if package_id_by_path
                .insert(package.package_path.clone(), package.id)
                .is_some()
            {
                return Err(DiscoveryError::new(
                    format!("duplicate package path '{}'", package.package_path),
                    Some(package.root_directory.clone()),
                ));
            }
        }
        Ok(Self {
            root_directory,
            packages,
            package_id_by_path,
        })
    }

    #[must_use]
    pub fn root_directory(&self) -> &Path {
        &self.root_directory
    }

    #[must_use]
    pub fn packages(&self) -> &[DiscoveredPackage] {
        &self.packages
    }

    #[must_use]
    pub fn package_by_path(&self, package_path: &str) -> Option<&DiscoveredPackage> {
        let package_id = self.package_id_by_path.get(package_path)?;
        self.packages.get(package_id.0)
    }
}
