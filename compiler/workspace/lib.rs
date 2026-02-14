mod discovery;
mod types;

pub use discovery::discover_workspace;
pub use types::{DiscoveredPackage, DiscoveryError, Workspace};
