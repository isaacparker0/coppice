use pkg_a::hello;

pub fn greet() -> &'static str {
    hello()
}
