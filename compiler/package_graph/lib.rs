use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use compiler__diagnostics::PhaseDiagnostic;
use compiler__source::Span;
use compiler__symbols::PackageDiagnostic;
use compiler__visibility::ResolvedImport;

type ImportAdjacencyByPackage = BTreeMap<String, BTreeSet<String>>;
type ImportSiteByEdge = BTreeMap<(String, String), ImportSite>;

pub fn check_cycles(resolved_imports: &[ResolvedImport], diagnostics: &mut Vec<PackageDiagnostic>) {
    let (adjacency_by_package, first_import_site_by_edge) =
        import_adjacency_and_first_site_by_edge(resolved_imports);

    let Some(cycle) = first_cycle_in_graph(&adjacency_by_package) else {
        return;
    };
    if cycle.len() < 2 {
        return;
    }

    let source = &cycle[0];
    let target = &cycle[1];
    let Some(import_site) = first_import_site_by_edge.get(&(source.clone(), target.clone())) else {
        return;
    };

    let cycle_display = cycle
        .iter()
        .map(|package| {
            if package.is_empty() {
                "workspace".to_string()
            } else {
                format!("workspace/{package}")
            }
        })
        .collect::<Vec<String>>()
        .join(" -> ");
    diagnostics.push(PackageDiagnostic {
        path: import_site.path.clone(),
        diagnostic: PhaseDiagnostic::new(
            format!("package import cycle detected: {cycle_display}"),
            import_site.span.clone(),
        ),
    });
}

#[must_use]
pub fn package_paths_in_cycle(resolved_imports: &[ResolvedImport]) -> BTreeSet<String> {
    let (adjacency_by_package, _) = import_adjacency_and_first_site_by_edge(resolved_imports);
    let Some(cycle) = first_cycle_in_graph(&adjacency_by_package) else {
        return BTreeSet::new();
    };
    if cycle.len() < 2 {
        return BTreeSet::new();
    }
    cycle[..cycle.len() - 1].iter().cloned().collect()
}

fn import_adjacency_and_first_site_by_edge(
    resolved_imports: &[ResolvedImport],
) -> (ImportAdjacencyByPackage, ImportSiteByEdge) {
    let mut adjacency_by_package: ImportAdjacencyByPackage = BTreeMap::new();
    let mut first_import_site_by_edge: ImportSiteByEdge = BTreeMap::new();

    for import in resolved_imports {
        adjacency_by_package
            .entry(import.source_package_path.clone())
            .or_default();
        adjacency_by_package
            .entry(import.target_package_path.clone())
            .or_default();

        if import.source_package_path == import.target_package_path {
            continue;
        }

        adjacency_by_package
            .entry(import.source_package_path.clone())
            .or_default()
            .insert(import.target_package_path.clone());
        first_import_site_by_edge
            .entry((
                import.source_package_path.clone(),
                import.target_package_path.clone(),
            ))
            .or_insert_with(|| ImportSite {
                path: import.source_path.clone(),
                span: import.import_span.clone(),
            });
    }

    (adjacency_by_package, first_import_site_by_edge)
}

#[derive(Clone)]
struct ImportSite {
    path: PathBuf,
    span: Span,
}

fn first_cycle_in_graph(adjacency_by_package: &ImportAdjacencyByPackage) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum VisitState {
        Visiting,
        Visited,
    }

    fn depth_first_search(
        node: &str,
        adjacency_by_package: &ImportAdjacencyByPackage,
        state_by_node: &mut BTreeMap<String, VisitState>,
        stack: &mut Vec<String>,
        index_by_node_in_stack: &mut BTreeMap<String, usize>,
    ) -> Option<Vec<String>> {
        state_by_node.insert(node.to_string(), VisitState::Visiting);
        index_by_node_in_stack.insert(node.to_string(), stack.len());
        stack.push(node.to_string());

        if let Some(neighbors) = adjacency_by_package.get(node) {
            for neighbor in neighbors {
                if let Some(index) = index_by_node_in_stack.get(neighbor) {
                    let mut cycle = stack[*index..].to_vec();
                    cycle.push(neighbor.clone());
                    return Some(cycle);
                }
                if state_by_node.get(neighbor) == Some(&VisitState::Visited) {
                    continue;
                }
                if let Some(cycle) = depth_first_search(
                    neighbor,
                    adjacency_by_package,
                    state_by_node,
                    stack,
                    index_by_node_in_stack,
                ) {
                    return Some(cycle);
                }
            }
        }

        stack.pop();
        index_by_node_in_stack.remove(node);
        state_by_node.insert(node.to_string(), VisitState::Visited);
        None
    }

    let mut state_by_node: BTreeMap<String, VisitState> = BTreeMap::new();
    let mut stack = Vec::new();
    let mut index_by_node_in_stack: BTreeMap<String, usize> = BTreeMap::new();

    for package in adjacency_by_package.keys() {
        if state_by_node.contains_key(package) {
            continue;
        }
        if let Some(cycle) = depth_first_search(
            package,
            adjacency_by_package,
            &mut state_by_node,
            &mut stack,
            &mut index_by_node_in_stack,
        ) {
            return Some(cycle);
        }
    }
    None
}
