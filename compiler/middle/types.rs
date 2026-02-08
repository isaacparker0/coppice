#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Integer64,
    Boolean,
    String,
    Unknown,
}

impl Type {
    pub fn name(&self) -> &'static str {
        match self {
            Type::Integer64 => "int64",
            Type::Boolean => "boolean",
            Type::String => "string",
            Type::Unknown => "<unknown>",
        }
    }
}

pub fn type_from_name(name: &str) -> Option<Type> {
    match name {
        "int64" => Some(Type::Integer64),
        "boolean" => Some(Type::Boolean),
        "string" => Some(Type::String),
        _ => None,
    }
}
