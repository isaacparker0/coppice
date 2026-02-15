use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use compiler__packages::PackageId;
use compiler__source::{FileId, FileRole, SourceFile, compare_paths};

use crate::types::{DiscoveredPackage, DiscoveryError, Workspace};

pub fn discover_workspace(root_directory: &Path) -> Result<Workspace, Vec<DiscoveryError>> {
    let mut package_roots = BTreeSet::new();
    let mut source_paths = Vec::new();
    let mut errors = Vec::new();

    if let Err(error) = collect_workspace_entries(
        root_directory,
        Path::new(""),
        &mut package_roots,
        &mut source_paths,
        &mut errors,
    ) {
        errors.push(DiscoveryError::new(
            format!("failed to walk workspace: {error}"),
            None,
        ));
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    source_paths.sort_by(|left, right| compare_paths(left, right));

    let mut source_paths_by_package_root: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for source_path in source_paths {
        let role = FileRole::from_path(&source_path).expect("source path must be .coppice");
        if role == FileRole::PackageManifest {
            continue;
        }
        let source_directory = source_path.parent().unwrap_or(Path::new(""));
        if let Some(package_root) = nearest_package_root(source_directory, &package_roots) {
            source_paths_by_package_root
                .entry(package_root)
                .or_default()
                .push(source_path);
        } else {
            errors.push(DiscoveryError::new(
                "source file is not owned by any package",
                Some(source_path),
            ));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    let mut file_id_counter = 0usize;
    let mut packages = Vec::new();
    for (package_index, package_root) in package_roots.iter().enumerate() {
        let mut source_files = Vec::new();
        if let Some(paths) = source_paths_by_package_root.get(package_root) {
            for source_path in paths {
                let role = FileRole::from_path(source_path).expect("source path must be .coppice");
                let source_file = SourceFile {
                    id: FileId(file_id_counter),
                    workspace_relative_path: source_path.clone(),
                    role,
                };
                file_id_counter += 1;
                source_files.push(source_file);
            }
        }

        let manifest_path = package_root.join("PACKAGE.coppice");
        packages.push(DiscoveredPackage {
            id: PackageId(package_index),
            package_path: package_path_from_root(package_root),
            root_directory: package_root.clone(),
            manifest_path,
            source_files,
        });
    }

    Workspace::new(root_directory.to_path_buf(), packages).map_err(|error| vec![error])
}

fn collect_workspace_entries(
    workspace_root: &Path,
    relative_directory: &Path,
    package_roots: &mut BTreeSet<PathBuf>,
    source_paths: &mut Vec<PathBuf>,
    errors: &mut Vec<DiscoveryError>,
) -> io::Result<()> {
    let absolute_directory = workspace_root.join(relative_directory);
    let mut entries = Vec::new();
    for entry in fs::read_dir(absolute_directory)? {
        entries.push(entry?);
    }
    entries.sort_by(|left, right| compare_paths(&left.path(), &right.path()));

    for entry in entries {
        let mut file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let metadata = match fs::metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    // Ignore dangling symlinks while walking the workspace tree.
                    continue;
                }
                Err(error) => {
                    return Err(error);
                }
            };
            file_type = metadata.file_type();
        }

        let file_name = entry.file_name();
        let child_relative_path = relative_directory.join(file_name);

        if file_type.is_dir() {
            collect_workspace_entries(
                workspace_root,
                &child_relative_path,
                package_roots,
                source_paths,
                errors,
            )?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let Some(role) = FileRole::from_path(&child_relative_path) else {
            continue;
        };
        source_paths.push(child_relative_path.clone());
        if role == FileRole::PackageManifest
            && !package_roots.insert(relative_directory.to_path_buf())
        {
            errors.push(DiscoveryError::new(
                "duplicate PACKAGE.coppice in package root",
                Some(child_relative_path),
            ));
        }
    }
    Ok(())
}

fn nearest_package_root(directory: &Path, package_roots: &BTreeSet<PathBuf>) -> Option<PathBuf> {
    let mut current = directory.to_path_buf();
    loop {
        if package_roots.contains(&current) {
            return Some(current);
        }
        match current.parent() {
            Some(parent) => {
                current = parent.to_path_buf();
            }
            None => {
                return None;
            }
        }
    }
}

fn package_path_from_root(root_directory: &Path) -> String {
    let key = compiler__source::path_to_key(root_directory);
    if key == "." || key.is_empty() {
        return String::new();
    }
    key
}
