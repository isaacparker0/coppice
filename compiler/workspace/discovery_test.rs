use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use compiler__source::FileRole;
use compiler__workspace::discover_workspace;

#[test]
fn assigns_files_to_nearest_manifest_package() {
    let workspace = TestWorkspace::new(&[
        "platform/auth/PACKAGE.copp",
        "platform/auth/token.copp",
        "platform/auth/crypto/hash.copp",
        "platform/auth/oauth/PACKAGE.copp",
        "platform/auth/oauth/google.copp",
    ]);

    let workspace = discover_workspace(workspace.path()).expect("discovery should succeed");

    let auth_package = workspace
        .package_by_path("platform/auth")
        .expect("auth package should exist");
    let auth_file_paths: Vec<String> = auth_package
        .source_files
        .iter()
        .map(|file| compiler__source::path_to_key(&file.workspace_relative_path))
        .collect();
    assert_eq!(
        auth_file_paths,
        vec![
            "platform/auth/crypto/hash.copp".to_string(),
            "platform/auth/token.copp".to_string(),
        ]
    );

    let oauth_package = workspace
        .package_by_path("platform/auth/oauth")
        .expect("oauth package should exist");
    let oauth_file_paths: Vec<String> = oauth_package
        .source_files
        .iter()
        .map(|file| compiler__source::path_to_key(&file.workspace_relative_path))
        .collect();
    assert_eq!(
        oauth_file_paths,
        vec!["platform/auth/oauth/google.copp".to_string()]
    );
}

#[test]
fn ignores_orphan_source_file() {
    let workspace = TestWorkspace::new(&["orphans/lost.copp"]);

    let discovered_workspace =
        discover_workspace(workspace.path()).expect("discovery should succeed");
    assert!(discovered_workspace.packages().is_empty());
}

#[test]
fn classifies_file_roles_by_suffix() {
    let workspace = TestWorkspace::new(&[
        "pkg/PACKAGE.copp",
        "pkg/lib.copp",
        "pkg/tool.bin.copp",
        "pkg/tool.test.copp",
    ]);

    let model = discover_workspace(workspace.path()).expect("discovery should succeed");
    let package = model
        .packages()
        .first()
        .expect("one package should exist for role test");

    let role_by_path: std::collections::BTreeMap<String, FileRole> = package
        .source_files
        .iter()
        .map(|file| {
            (
                compiler__source::path_to_key(&file.workspace_relative_path),
                file.role,
            )
        })
        .collect();

    assert_eq!(role_by_path.get("pkg/lib.copp"), Some(&FileRole::Library));
    assert_eq!(
        role_by_path.get("pkg/tool.bin.copp"),
        Some(&FileRole::BinaryEntrypoint)
    );
    assert_eq!(
        role_by_path.get("pkg/tool.test.copp"),
        Some(&FileRole::Test)
    );
}

#[test]
fn discovery_order_is_deterministic() {
    let workspace = TestWorkspace::new(&[
        "zeta/PACKAGE.copp",
        "zeta/c.copp",
        "zeta/a.copp",
        "alpha/PACKAGE.copp",
        "alpha/b.copp",
    ]);

    let first = discover_workspace(workspace.path()).expect("first discovery should succeed");
    let second = discover_workspace(workspace.path()).expect("second discovery should succeed");

    let first_paths: Vec<String> = first
        .packages()
        .iter()
        .map(|package| package.package_path.clone())
        .collect();
    let second_paths: Vec<String> = second
        .packages()
        .iter()
        .map(|package| package.package_path.clone())
        .collect();
    assert_eq!(first_paths, second_paths);

    let first_source_paths: Vec<String> = first
        .packages()
        .iter()
        .flat_map(|package| package.source_files.iter())
        .map(|file| compiler__source::path_to_key(&file.workspace_relative_path))
        .collect();
    let second_source_paths: Vec<String> = second
        .packages()
        .iter()
        .flat_map(|package| package.source_files.iter())
        .map(|file| compiler__source::path_to_key(&file.workspace_relative_path))
        .collect();
    assert_eq!(first_source_paths, second_source_paths);
}

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new(files: &[&str]) -> Self {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("coppice_workspace_test_{unique_suffix}"));
        fs::create_dir_all(&root).expect("workspace root should be created");

        for relative_file in files {
            let path = root.join(relative_file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory should be created");
            }
            fs::write(path, "").expect("test file should be written");
        }

        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
