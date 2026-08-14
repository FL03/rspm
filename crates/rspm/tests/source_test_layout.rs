//! Standalone regression for RSPM's Rust test-placement contract.

use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::{
    Attribute, Item, Meta,
    punctuated::Punctuated,
    token::Comma,
    visit::{self, Visit},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Possibility {
    can_be_false: bool,
    can_be_true: bool,
}

impl Possibility {
    const BOTH: Self = Self {
        can_be_false: true,
        can_be_true: true,
    };
    const FALSE: Self = Self {
        can_be_false: true,
        can_be_true: false,
    };
    const TRUE: Self = Self {
        can_be_false: false,
        can_be_true: true,
    };

    fn and(self, other: Self) -> Self {
        Self {
            can_be_false: self.can_be_false || other.can_be_false,
            can_be_true: self.can_be_true && other.can_be_true,
        }
    }

    fn or(self, other: Self) -> Self {
        Self {
            can_be_false: self.can_be_false && other.can_be_false,
            can_be_true: self.can_be_true || other.can_be_true,
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            can_be_false: self.can_be_false || other.can_be_false,
            can_be_true: self.can_be_true || other.can_be_true,
        }
    }

    const fn not(self) -> Self {
        Self {
            can_be_false: self.can_be_true,
            can_be_true: self.can_be_false,
        }
    }
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    fn visit(directory: &Path, sources: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let path = entry.expect("source directory entry").path();
            if path.is_dir() {
                visit(&path, sources);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    let mut sources = Vec::new();
    visit(root, &mut sources);
    sources.sort();
    sources
}

fn nested_meta(list: &syn::MetaList) -> Result<Punctuated<Meta, Comma>, String> {
    list.parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)
        .map_err(|error| format!("parse cfg predicate: {error}"))
}

fn cfg_possibility(meta: &Meta) -> Result<Possibility, String> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Ok(Possibility::FALSE),
        Meta::Path(_) | Meta::NameValue(_) => Ok(Possibility::BOTH),
        Meta::List(list) if list.path.is_ident("all") => nested_meta(list)?
            .iter()
            .try_fold(Possibility::TRUE, |possibility, meta| {
                Ok(possibility.and(cfg_possibility(meta)?))
            }),
        Meta::List(list) if list.path.is_ident("any") => nested_meta(list)?
            .iter()
            .try_fold(Possibility::FALSE, |possibility, meta| {
                Ok(possibility.or(cfg_possibility(meta)?))
            }),
        Meta::List(list) if list.path.is_ident("not") => {
            let predicates = nested_meta(list)?;
            if predicates.len() != 1 {
                return Err(format!(
                    "cfg(not(...)) must contain one predicate, found {}",
                    predicates.len()
                ));
            }
            Ok(cfg_possibility(&predicates[0])?.not())
        }
        Meta::List(_) => Ok(Possibility::BOTH),
    }
}

fn cfg_meta_attribute_possibility(meta: &Meta) -> Result<Possibility, String> {
    if let Meta::List(list) = meta {
        if list.path.is_ident("cfg") {
            let predicate = list
                .parse_args::<Meta>()
                .map_err(|error| format!("parse nested cfg attribute: {error}"))?;
            return cfg_possibility(&predicate);
        }
        if list.path.is_ident("cfg_attr") {
            return cfg_attr_possibility(list);
        }
    }
    Ok(Possibility::TRUE)
}

fn cfg_attr_possibility(list: &syn::MetaList) -> Result<Possibility, String> {
    let arguments = nested_meta(list)?;
    let Some(condition) = arguments.first() else {
        return Err("cfg_attr requires a condition".to_owned());
    };
    let condition = cfg_possibility(condition)?;
    let applied = arguments
        .iter()
        .skip(1)
        .try_fold(Possibility::TRUE, |possibility, meta| {
            Ok::<_, String>(possibility.and(cfg_meta_attribute_possibility(meta)?))
        })?;

    let mut result = Possibility {
        can_be_false: false,
        can_be_true: false,
    };
    if condition.can_be_false {
        result = result.union(Possibility::TRUE);
    }
    if condition.can_be_true {
        result = result.union(applied);
    }
    Ok(result)
}

fn cfg_requires_test(attributes: &[Attribute]) -> Result<bool, String> {
    let possibility = attributes
        .iter()
        .try_fold(Possibility::TRUE, |possibility, attribute| {
            let attribute_possibility = if attribute.path().is_ident("cfg") {
                let predicate = attribute
                    .parse_args::<Meta>()
                    .map_err(|error| format!("parse cfg attribute: {error}"))?;
                cfg_possibility(&predicate)?
            } else if attribute.path().is_ident("cfg_attr") {
                let list = attribute
                    .meta
                    .require_list()
                    .map_err(|error| error.to_string())?;
                cfg_attr_possibility(list)?
            } else {
                Possibility::TRUE
            };
            Ok::<_, String>(possibility.and(attribute_possibility))
        })?;
    Ok(!possibility.can_be_true)
}

fn meta_contains_path(meta: &Meta) -> Result<bool, String> {
    match meta {
        Meta::Path(path) | Meta::NameValue(syn::MetaNameValue { path, .. }) => {
            Ok(path.is_ident("path"))
        }
        Meta::List(list) if list.path.is_ident("path") => Ok(true),
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            for meta in nested_meta(list)?.iter().skip(1) {
                if meta_contains_path(meta)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Meta::List(_) => Ok(false),
    }
}

fn has_path_override(attributes: &[Attribute]) -> Result<bool, String> {
    for attribute in attributes {
        if attribute.path().is_ident("path") {
            return Ok(true);
        }
        if attribute.path().is_ident("cfg_attr") {
            let arguments = attribute
                .parse_args_with(Punctuated::<Meta, Comma>::parse_terminated)
                .map_err(|error| format!("parse cfg attribute: {error}"))?;
            for meta in arguments.iter().skip(1) {
                if meta_contains_path(meta)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn item_attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn flatten_tokens(stream: TokenStream, flattened: &mut Vec<String>) {
    for token in stream {
        match token {
            TokenTree::Group(group) => flatten_tokens(group.stream(), flattened),
            TokenTree::Ident(identifier) => flattened.push(identifier.to_string()),
            TokenTree::Punct(punctuation) => flattened.push(punctuation.as_char().to_string()),
            TokenTree::Literal(_) => flattened.push("<literal>".to_owned()),
        }
    }
}

fn macro_may_generate_external_module(item: &syn::ItemMacro, test_only: bool) -> bool {
    let mut tokens = Vec::new();
    flatten_tokens(item.mac.tokens.clone(), &mut tokens);
    let identifier = |token: &str| {
        token
            .strip_prefix("r#")
            .unwrap_or(token)
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_alphabetic())
    };
    let has_external_module = tokens
        .windows(3)
        .any(|window| window[0] == "mod" && identifier(&window[1]) && window[2] == ";")
        || tokens.windows(4).any(|window| {
            window[0] == "mod" && window[1] == "$" && identifier(&window[2]) && window[3] == ";"
        });
    has_external_module
        && (test_only
            || (tokens.iter().any(|token| token == "cfg")
                && tokens.iter().any(|token| token == "test")))
}

#[derive(Default)]
struct LayoutVisitor {
    test_only: Vec<bool>,
    violations: Vec<String>,
    errors: Vec<String>,
}

impl LayoutVisitor {
    fn current_test_only(&self) -> bool {
        self.test_only.last().copied().unwrap_or(false)
    }
}

impl<'ast> Visit<'ast> for LayoutVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        let inherited = self.current_test_only();
        let item_test_only = match cfg_requires_test(item_attributes(item)) {
            Ok(required) => inherited || required,
            Err(error) => {
                self.errors.push(error);
                true
            }
        };
        self.test_only.push(item_test_only);
        visit::visit_item(self, item);
        self.test_only.pop();
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.content.is_none() {
            let path_override = match has_path_override(&module.attrs) {
                Ok(path_override) => path_override,
                Err(error) => {
                    self.errors.push(error);
                    true
                }
            };
            if self.current_test_only() || path_override {
                self.violations
                    .push(format!("external module `{}`", module.ident));
            }
        }
        visit::visit_item_mod(self, module);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if macro_may_generate_external_module(item, self.current_test_only()) {
            self.violations
                .push("macro-generated external test module".to_owned());
        }
        visit::visit_item_macro(self, item);
    }
}

fn test_layout_violations(source: &str) -> Result<Vec<String>, String> {
    let file = syn::parse_file(source).map_err(|error| format!("parse Rust source: {error}"))?;
    let mut visitor = LayoutVisitor::default();
    visitor.visit_file(&file);
    if visitor.errors.is_empty() {
        Ok(visitor.violations)
    } else {
        Err(visitor.errors.join("; "))
    }
}

/// [REGRESSION][EVAL] Private unit tests stay literal inline modules. Public
/// black-box tests live under `tests/`; neither form needs a source-side test
/// module file or `#[path]` escape hatch.
#[test]
fn source_tree_contains_no_standalone_test_modules() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(source_root.is_dir(), "RSPM source root must exist");
    let sources = rust_sources(&source_root);
    assert!(
        !sources.is_empty(),
        "RSPM source discovery must fail closed"
    );

    for path in sources {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 Rust source name");
        assert!(
            name != "tests.rs" && !name.ends_with("_test.rs") && !name.ends_with("_tests.rs"),
            "standalone test module must be inline or under tests/: {}",
            path.display()
        );
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let violations = test_layout_violations(&source)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(
            violations.is_empty(),
            "test-only external modules are forbidden in {}: {violations:?}",
            path.display()
        );
    }
}

/// [REGRESSION][EVAL] Nightly's current atomic update API stays warning-free;
/// strict clippy promotes a reintroduction of the deprecated spelling to an
/// error before Axiom consumes this nested crate.
#[test]
fn source_tree_uses_current_atomic_update_api() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources = rust_sources(&source_root);
    assert!(
        !sources.is_empty(),
        "RSPM source discovery must fail closed"
    );

    for path in sources {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(
            !source.contains(".fetch_update("),
            "deprecated atomic update API in {}; use `try_update`",
            path.display()
        );
    }
}

#[test]
fn semantic_scan_rejects_renamed_nested_raw_and_path_evasions() {
    for mutated in [
        "#[cfg(test)] mod checks;",
        "#[cfg(test)] mod r#checks;",
        "#[cfg(all(\n test,\n feature = \"sdk\"\n))] mod tests { mod helper; }",
        "# [ cfg(test) ] # [ path = \"checks.rs\" ] mod renamed;",
        "#[cfg(test)] #[cfg_attr(test, path = \"checks.rs\")] mod renamed;",
        "#[cfg_attr(not(test), cfg(any()))] mod checks;",
        "#[cfg(test)] fn fixture() { if true { mod checks; } }",
        "#[cfg(test)] macro_rules! external { () => { mod checks; } }",
        "#[cfg(test)] macro_rules! external { ($name:ident) => { mod $name; } }",
        "#[cfg_attr(test, cfg_attr(test, path = \"checks.rs\"))] mod renamed;",
    ] {
        let violations = test_layout_violations(mutated).expect("parse mutation");
        assert_eq!(
            violations.len(),
            1,
            "mutation must produce exactly one violation: {mutated}"
        );
    }
}

#[test]
fn semantic_scan_accepts_inline_tests_and_production_cfg_alternatives() {
    for accepted in [
        "#[cfg(test)] mod tests { #[test] fn works() {} }",
        "#[cfg(any(feature = \"serve\", feature = \"test-utils\"))] mod service;",
        "#[cfg(feature = \"serve\")] mod production;",
    ] {
        assert_eq!(
            test_layout_violations(accepted).expect("parse accepted source"),
            Vec::<String>::new(),
            "valid production or inline-test layout must remain accepted: {accepted}"
        );
    }
}

#[test]
fn semantic_scan_fails_closed_on_invalid_rust() {
    assert!(test_layout_violations("#[cfg(test)] mod").is_err());
}
