use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub name: String,
}

pub fn create_runtime() -> Runtime {
    Runtime::new().unwrap()
}
