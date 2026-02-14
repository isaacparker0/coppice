use std::collections::{HashSet, VecDeque};
use std::error::Error;
use syn::parse_file;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};

pub struct SourceInfo {
    pub imports: Vec<String>,
    pub external_modules: Vec<String>,
    pub has_main: bool,
}

pub fn parse_source(contents: &str) -> Result<SourceInfo, Box<dyn Error>> {
    let ast = parse_file(contents)?;
    let mut visitor = AstVisitor::default();
    visitor.visit_file(&ast);

    let mut root_scope = visitor.mod_stack.pop_back().expect("no root scope");
    assert!(visitor.mod_stack.is_empty(), "leftover scopes");

    root_scope.trim_early_imports();

    Ok(SourceInfo {
        imports: filter_imports(root_scope.imports),
        external_modules: visitor.extern_mods,
        has_main: visitor.has_main,
    })
}

const PRIMITIVES: &[&str] = &[
    "bool", "char", "str", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64",
    "u128", "usize", "f32", "f64",
];

fn filter_imports(imports: Vec<Ident>) -> Vec<String> {
    imports
        .into_iter()
        .filter_map(|ident| {
            let s = ident.to_string();
            // Uppercase identifiers are types/structs, not crate imports.
            if !s.chars().next().is_some_and(char::is_lowercase) {
                return None;
            }
            // Primitive types are never crate imports.
            if PRIMITIVES.contains(&s.as_str()) {
                return None;
            }
            Some(s)
        })
        .collect()
}

/// Identifier wrapper that handles both borrowed and owned syn::Ident values.
/// Macros produce owned values while normal AST traversal produces references.
#[derive(Debug, Clone)]
enum Ident<'ast> {
    Ref(&'ast syn::Ident),
    Owned(syn::Ident),
}

impl<'ast> From<&'ast syn::Ident> for Ident<'ast> {
    fn from(ident: &'ast syn::Ident) -> Self {
        Self::Ref(ident)
    }
}

impl From<syn::Ident> for Ident<'_> {
    fn from(ident: syn::Ident) -> Self {
        Self::Owned(ident)
    }
}

// Compare by string value, not by variant. This is needed because macro body
// parsing produces Owned idents that must be compared against Ref idents from
// the normal AST traversal.
impl PartialEq for Ident<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.ident() == other.ident()
    }
}

impl Eq for Ident<'_> {}

impl std::hash::Hash for Ident<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.ident().hash(state);
    }
}

impl PartialEq<&str> for Ident<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.ident() == *other
    }
}

impl Ident<'_> {
    fn ident(&self) -> &syn::Ident {
        match self {
            Self::Ref(ident) => ident,
            Self::Owned(ident) => ident,
        }
    }

    #[allow(clippy::inherent_to_string)]
    fn to_string(&self) -> String {
        self.ident().to_string()
    }
}

#[derive(Debug, Default)]
struct Scope<'ast> {
    mods: HashSet<Ident<'ast>>,
    imports: Vec<Ident<'ast>>,
}

impl Scope<'_> {
    /// Remove imports for mods that were defined after the import appeared.
    fn trim_early_imports(&mut self) {
        self.imports.retain(|import| !self.mods.contains(import));
    }
}

#[derive(Debug)]
struct AstVisitor<'ast> {
    mod_stack: VecDeque<Scope<'ast>>,
    /// All mods in scope, including from parent scopes
    scope_mods: HashSet<Ident<'ast>>,
    /// `mod foo;` declarations (files to include in crate)
    extern_mods: Vec<String>,
    /// Prevents use statement items from shadowing their own crate import
    mod_denylist: HashSet<Ident<'ast>>,
    has_main: bool,
}

impl Default for AstVisitor<'_> {
    fn default() -> Self {
        let mut mod_stack = VecDeque::new();
        mod_stack.push_back(Scope::default());
        Self {
            mod_stack,
            scope_mods: HashSet::default(),
            extern_mods: Vec::default(),
            mod_denylist: HashSet::new(),
            has_main: false,
        }
    }
}

impl<'ast> AstVisitor<'ast> {
    fn add_import<I: Into<Ident<'ast>>>(&mut self, ident: I) {
        let ident = ident.into();

        if ident == "crate" || ident == "super" || ident == "self" {
            return;
        }

        if !self.scope_mods.contains(&ident) {
            self.mod_stack.back_mut().unwrap().imports.push(ident);
        }
    }

    /// Macro bodies are stored as TokenStream and not visited by syn's AST
    /// traversal.
    fn extract_paths_from_expr(&mut self, expr: &syn::Expr) {
        match expr {
            syn::Expr::Path(expr_path) => {
                if expr_path.path.segments.len() > 1 {
                    self.add_import(expr_path.path.segments[0].ident.clone());
                }
            }
            syn::Expr::Call(call) => {
                self.extract_paths_from_expr(&call.func);
                for arg in &call.args {
                    self.extract_paths_from_expr(arg);
                }
            }
            syn::Expr::MethodCall(method) => {
                self.extract_paths_from_expr(&method.receiver);
                for arg in &method.args {
                    self.extract_paths_from_expr(arg);
                }
            }
            syn::Expr::Binary(binary) => {
                self.extract_paths_from_expr(&binary.left);
                self.extract_paths_from_expr(&binary.right);
            }
            syn::Expr::Unary(unary) => {
                self.extract_paths_from_expr(&unary.expr);
            }
            syn::Expr::Reference(reference) => {
                self.extract_paths_from_expr(&reference.expr);
            }
            syn::Expr::Paren(parenthesis) => {
                self.extract_paths_from_expr(&parenthesis.expr);
            }
            syn::Expr::Field(field) => {
                self.extract_paths_from_expr(&field.base);
            }
            syn::Expr::Index(index) => {
                self.extract_paths_from_expr(&index.expr);
                self.extract_paths_from_expr(&index.index);
            }
            syn::Expr::Tuple(tuple) => {
                for elem in &tuple.elems {
                    self.extract_paths_from_expr(elem);
                }
            }
            syn::Expr::Array(array) => {
                for elem in &array.elems {
                    self.extract_paths_from_expr(elem);
                }
            }
            syn::Expr::Cast(cast) => {
                self.extract_paths_from_expr(&cast.expr);
            }
            syn::Expr::If(expr_if) => {
                self.extract_paths_from_expr(&expr_if.cond);
                for stmt in &expr_if.then_branch.stmts {
                    if let syn::Stmt::Expr(e, _) = stmt {
                        self.extract_paths_from_expr(e);
                    }
                }
                if let Some((_, else_branch)) = &expr_if.else_branch {
                    self.extract_paths_from_expr(else_branch);
                }
            }
            syn::Expr::Match(expr_match) => {
                self.extract_paths_from_expr(&expr_match.expr);
                for arm in &expr_match.arms {
                    self.extract_paths_from_expr(&arm.body);
                }
            }
            syn::Expr::Block(block) => {
                for stmt in &block.block.stmts {
                    if let syn::Stmt::Expr(e, _) = stmt {
                        self.extract_paths_from_expr(e);
                    }
                }
            }
            syn::Expr::Closure(closure) => {
                self.extract_paths_from_expr(&closure.body);
            }
            syn::Expr::Struct(expr_struct) => {
                if expr_struct.path.segments.len() > 1 {
                    self.add_import(expr_struct.path.segments[0].ident.clone());
                }
                for field in &expr_struct.fields {
                    self.extract_paths_from_expr(&field.expr);
                }
            }
            // Skip literals, breaks, continues, etc.; they don't contain paths.
            _ => {}
        }
    }

    fn add_mod<I: Into<Ident<'ast>>>(&mut self, ident: I) {
        let ident = ident.into();

        if !self.scope_mods.contains(&ident) && !self.mod_denylist.contains(&ident) {
            self.scope_mods.insert(ident.clone());
            self.mod_stack.back_mut().unwrap().mods.insert(ident);
        }
    }

    fn push_scope(&mut self) {
        self.mod_stack.push_back(Scope::default());
    }

    fn pop_scope(&mut self) {
        let mut scope = self.mod_stack.pop_back().expect("hit bottom of stack");

        for rename in &scope.mods {
            self.scope_mods.remove(rename);
        }

        scope.trim_early_imports();

        let parent_scope = self.mod_stack.back_mut().expect("no parent scope");
        parent_scope.imports.extend(scope.imports);
    }

    fn is_root_scope(&self) -> bool {
        self.mod_stack.len() == 1
    }

    fn visit_attr_meta(&mut self, meta: &syn::Meta) {
        match meta {
            syn::Meta::Path(path) => {
                if path.segments.len() > 1 {
                    self.add_import(
                        path.segments
                            .pairs()
                            .next()
                            .unwrap()
                            .into_value()
                            .ident
                            .clone(),
                    );
                }
            }
            syn::Meta::List(list) => {
                if let Some(ident) = list.path.get_ident() {
                    if ident == "derive" {
                        if let Ok(nested) = list.parse_args_with(
                            Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                        ) {
                            for derive in nested {
                                self.visit_attr_meta(&derive);
                            }
                        }
                    } else if ident == "cfg_attr"
                        && let Ok(nested) = list.parse_args_with(
                            Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                        )
                    {
                        let mut iter = nested.into_iter();
                        iter.next();
                        if let Some(inner) = iter.next() {
                            self.visit_attr_meta(&inner);
                        }
                    }
                }
            }
            syn::Meta::NameValue(_) => (),
        }
    }
}

impl<'ast> Visit<'ast> for AstVisitor<'ast> {
    fn visit_use_name(&mut self, node: &'ast syn::UseName) {
        self.add_mod(&node.ident);
    }

    fn visit_use_rename(&mut self, node: &'ast syn::UseRename) {
        self.add_import(&node.ident);
        self.add_mod(&node.rename);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        if node.segments.len() > 1 {
            self.add_import(&node.segments[0].ident);
        }
        visit::visit_path(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut imports = HashSet::new();
        parse_use_imports(&node.tree, &mut imports);

        for import in &imports {
            self.add_import(import.clone());
        }

        self.mod_denylist = imports;
        visit::visit_item_use(self, node);
        self.mod_denylist.clear();
    }

    fn visit_use_path(&mut self, node: &'ast syn::UsePath) {
        if let syn::UseTree::Group(group) = &*node.tree {
            for tree in &group.items {
                if let syn::UseTree::Name(name) = tree
                    && name.ident == "self"
                {
                    self.add_mod(&node.ident);
                }
            }
        }
        visit::visit_use_path(self, node);
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        self.add_import(&node.ident);
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        self.push_scope();
        visit::visit_block(self, node);
        self.pop_scope();
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        // External mod declarations indicate files to include in the crate.
        if self.is_root_scope() && node.content.is_none() {
            self.extern_mods.push(node.ident.to_string());
        }

        self.add_mod(&node.ident);
        self.push_scope();
        visit::visit_item_mod(self, node);
        self.pop_scope();
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if self.is_root_scope() && node.sig.ident == "main" {
            self.has_main = true;
        }

        self.push_scope();
        visit::visit_item_fn(self, node);
        self.pop_scope();
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        visit::visit_item_enum(self, node);
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        visit::visit_item_type(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        self.visit_attr_meta(&node.meta);
        visit::visit_attribute(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if let Some(macro_ident) = node.mac.path.get_ident()
            && macro_ident == "macro_rules"
            && let Some(new_ident) = &node.ident
        {
            self.add_mod(new_ident);
        }
        visit::visit_item_macro(self, node);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // Try to parse macro body as comma-separated expressions. This handles
        // common macros like format!, println!, vec!, assert!, etc.
        if let Ok(args) =
            mac.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        {
            for expr in &args {
                self.extract_paths_from_expr(expr);
            }
        }
        // If parsing fails, the macro has custom syntax; skip it gracefully.
        visit::visit_macro(self, mac);
    }
}

fn parse_use_imports<'ast>(use_tree: &'ast syn::UseTree, imports: &mut HashSet<Ident<'ast>>) {
    match use_tree {
        syn::UseTree::Path(path) => {
            imports.insert(Ident::Ref(&path.ident));
        }
        syn::UseTree::Name(name) => {
            imports.insert(Ident::Ref(&name.ident));
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                parse_use_imports(item, imports);
            }
        }
        syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => (),
    }
}
