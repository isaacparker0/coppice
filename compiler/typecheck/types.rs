#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Integer64,
    Boolean,
    String,
    Nil,
    Named(String),
    Union(Vec<Type>),
    Unknown,
}

impl Type {
    pub fn name(&self) -> &str {
        match self {
            Type::Integer64 => "int64",
            Type::Boolean => "boolean",
            Type::String => "string",
            Type::Nil => "nil",
            Type::Named(name) => name.as_str(),
            Type::Union(_) => "<union>",
            Type::Unknown => "<unknown>",
        }
    }

    pub fn display(&self) -> String {
        match self {
            Type::Union(types) => types
                .iter()
                .map(Type::display)
                .collect::<Vec<_>>()
                .join(" | "),
            _ => self.name().to_string(),
        }
    }
}

pub fn type_from_name(name: &str) -> Option<Type> {
    match name {
        "int64" => Some(Type::Integer64),
        "boolean" => Some(Type::Boolean),
        "string" => Some(Type::String),
        "nil" => Some(Type::Nil),
        _ => None,
    }
}
