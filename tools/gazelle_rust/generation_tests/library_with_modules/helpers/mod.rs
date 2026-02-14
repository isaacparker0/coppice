mod inner;

pub fn world() -> &'static str {
    inner::get_world()
}
