use std::hash::{Hash, Hasher};

use compiler__packages::PackageId;
use compiler__source::Span;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NominalTypeId {
    pub package_id: PackageId,
    pub symbol_name: String,
}

#[derive(Clone, Debug)]
pub struct NominalTypeRef {
    pub id: NominalTypeId,
    pub display_name: String,
}

impl PartialEq for NominalTypeRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for NominalTypeRef {}

impl Hash for NominalTypeRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Integer64,
    Boolean,
    String,
    Nil,
    Named(NominalTypeRef),
    Applied {
        base: NominalTypeRef,
        arguments: Vec<Type>,
    },
    TypeParameter(String),
    Union(Vec<Type>),
    Unknown,
}

impl Type {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Type::Integer64 => "int64",
            Type::Boolean => "boolean",
            Type::String => "string",
            Type::Nil => "nil",
            Type::Named(named) => named.display_name.as_str(),
            Type::Applied { .. } => "<applied>",
            Type::TypeParameter(name) => name,
            Type::Union(_) => "<union>",
            Type::Unknown => "<unknown>",
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Type::Applied { base, arguments } => {
                let joined = arguments
                    .iter()
                    .map(Type::display)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}[{joined}]", base.display_name)
            }
            Type::Union(types) => types
                .iter()
                .map(Type::display)
                .collect::<Vec<_>>()
                .join(" | "),
            _ => self.name().to_string(),
        }
    }
}

#[must_use]
pub fn type_from_builtin_name(name: &str) -> Option<Type> {
    match name {
        "int64" => Some(Type::Integer64),
        "boolean" => Some(Type::Boolean),
        "string" => Some(Type::String),
        "nil" => Some(Type::Nil),
        _ => None,
    }
}

#[derive(Clone)]
pub struct TypedFunctionSignature {
    pub type_parameters: Vec<String>,
    pub parameter_types: Vec<Type>,
    pub return_type: Type,
}

#[derive(Clone)]
pub enum ImportedTypeShape {
    Struct {
        fields: Vec<(String, Type)>,
        methods: Vec<ImportedMethodSignature>,
    },
    Union {
        variants: Vec<Type>,
    },
}

#[derive(Clone)]
pub struct ImportedTypeDeclaration {
    pub nominal_type_id: NominalTypeId,
    pub type_parameters: Vec<String>,
    pub kind: ImportedTypeShape,
}

#[derive(Clone)]
pub struct ImportedMethodSignature {
    pub name: String,
    pub self_mutable: bool,
    pub parameter_types: Vec<Type>,
    pub return_type: Type,
}

pub enum TypedSymbol {
    Type,
    Function(TypedFunctionSignature),
    Constant(Type),
}

#[derive(Default)]
pub struct FileTypecheckSummary {
    pub typed_symbol_by_name: std::collections::HashMap<String, TypedSymbol>,
}

#[derive(Clone)]
pub enum ImportedSymbol {
    Type(ImportedTypeDeclaration),
    Function(TypedFunctionSignature),
    Constant(Type),
}

#[derive(Clone)]
pub struct ImportedBinding {
    pub local_name: String,
    pub span: Span,
    pub symbol: ImportedSymbol,
}
