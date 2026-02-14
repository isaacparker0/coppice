mod helpers;
mod utils;

pub fn greet() -> String {
    format!("{} {}", utils::hello(), helpers::world())
}
