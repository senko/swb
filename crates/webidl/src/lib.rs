//! `swb-webidl`: a parser for the subset of Web IDL that swb uses and the
//! generator of the binding glue of `swb-web` (ADR 0028 section 5). It is
//! a development tool (`just bindings` runs the binary); the browser does
//! not depend on it.
//!
//! - `lexer` and `parser`: the IDL text to definitions (`ast`). Errors name
//!   the file and the line.
//! - `model`: the definitions of a set of files merged and checked;
//!   `legacy`: the special operations and `iterable` declarations.
//! - `emit`: the Rust modules.
//!
//! A *set* is a list of IDL files and the directory of its generated
//! modules ([`binding_sets`]): the IDL of the browser in `crates/web/idl/`,
//! and the test IDL of `swb-web`'s tests.

pub mod ast;
mod emit;
mod legacy;
mod lexer;
mod model;
mod parser;

use std::fmt;
use std::path::{Path, PathBuf};

pub use parser::parse;

/// An error in an IDL file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The file name.
    pub file: String,
    /// The line (1-based).
    pub line: u32,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

impl std::error::Error for Error {}

/// An IDL source file: its name and text.
#[derive(Clone, Debug)]
pub struct Source {
    /// The file name (for messages and the headers of the output).
    pub name: String,
    /// The text.
    pub text: String,
}

/// Generates the modules of a set of IDL files: `(file name, text)` pairs
/// in name order. `support` is the Rust path of the support module of
/// `swb-web` (`crate::bindings` inside the crate).
pub fn generate(sources: &[Source], support: &str) -> Result<Vec<(String, String)>, Error> {
    let mut definitions = Vec::new();
    for source in sources {
        definitions.extend(parse(&source.name, &source.text)?);
    }
    let model = model::Model::new(definitions)?;
    let names: Vec<String> = sources.iter().map(|s| s.name.clone()).collect();
    emit::generate(&model, support, &names)
}

/// A set of IDL files with its output directory, relative to the
/// repository root.
#[derive(Clone, Debug)]
pub struct BindingSet {
    /// The IDL files in order.
    pub idl: Vec<PathBuf>,
    /// The directory of the generated modules.
    pub out_dir: PathBuf,
    /// The Rust path of the support module.
    pub support: &'static str,
}

/// The sets of the repository: the browser's IDL (every `.idl` file of
/// `crates/web/idl/`, in name order) into `crates/web/src/generated/`,
/// and the test IDL of `swb-web` (with `webidl.idl` for `DOMException`)
/// into `crates/web/tests/generated/`.
pub fn binding_sets(root: &Path) -> std::io::Result<Vec<BindingSet>> {
    let idl_dir = Path::new("crates/web/idl");
    let mut idl: Vec<PathBuf> = std::fs::read_dir(root.join(idl_dir))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "idl"))
        .filter_map(|p| p.file_name().map(|n| idl_dir.join(n)))
        .collect();
    idl.sort();
    Ok(vec![
        BindingSet {
            idl,
            out_dir: PathBuf::from("crates/web/src/generated"),
            support: "crate::bindings",
        },
        BindingSet {
            idl: vec![
                PathBuf::from("crates/web/idl/webidl.idl"),
                PathBuf::from("crates/web/tests/idl/test.idl"),
            ],
            out_dir: PathBuf::from("crates/web/tests/generated"),
            support: "swb_web::bindings",
        },
    ])
}

/// Reads the IDL files of a set and generates its modules.
pub fn generate_set(root: &Path, set: &BindingSet) -> Result<Vec<(String, String)>, String> {
    let mut sources = Vec::new();
    for path in &set.idl {
        let text = std::fs::read_to_string(root.join(path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        sources.push(Source { name, text });
    }
    generate(&sources, set.support).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOM_EXCEPTION: &str = "[Exposed=*] interface DOMException { constructor(optional DOMString message = \"\", optional DOMString name = \"Error\"); readonly attribute DOMString name; };";

    fn files(idl: &str) -> Result<Vec<(String, String)>, Error> {
        generate(
            &[
                Source {
                    name: "webidl.idl".into(),
                    text: DOM_EXCEPTION.into(),
                },
                Source {
                    name: "t.idl".into(),
                    text: idl.into(),
                },
            ],
            "crate::bindings",
        )
    }

    fn file(idl: &str, name: &str) -> String {
        let files = files(idl).unwrap();
        files
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, text)| text)
            .unwrap()
    }

    fn error(idl: &str) -> String {
        files(idl).unwrap_err().to_string()
    }

    #[test]
    fn output_of_a_small_interface() {
        let idl = "[Exposed=Window] interface Node { const unsigned short ELEMENT_NODE = 1; readonly attribute Node? parentNode; attribute DOMString? textContent; Node appendChild(Node node); };\n[Exposed=Window] interface Element : Node { [Reflect] attribute DOMString id; };";
        let names: Vec<String> = files(idl).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            [
                "dom_exception.rs",
                "element.rs",
                "mod.rs",
                "node.rs",
                "types.rs"
            ]
        );
        let node = file(idl, "node.rs");
        assert!(node.starts_with("// Generated by swb-webidl from webidl.idl, t.idl."));
        assert!(node.contains("pub trait NodeImpl: b::Host {"));
        assert!(node.contains("    fn parent_node(cx: &mut b::Cx<'_>, this: b::NodeId) -> b::Result<Option<b::NodeId>>;"));
        assert!(node.contains("    fn set_text_content(cx: &mut b::Cx<'_>, this: b::NodeId, value: Option<String>) -> b::Result<()>;"));
        assert!(node.contains("    fn append_child(cx: &mut b::Cx<'_>, this: b::NodeId, node: b::NodeId) -> b::Result<b::NodeId>;"));
        assert!(node.contains("b::glue::require_arguments(call, 1)?;"));
        assert!(node.contains(
            "let a0 = b::convert::node(rt, v, super::NODE, \"Node\").map_err(|e| e.argument(0))?;"
        ));
        assert!(node.contains("rt.define_value(interface.object, \"ELEMENT_NODE\", swb_js::Value::Int(1), swb_js::Attributes::CONSTANT)?;"));
        assert!(node.contains("rt.define_method(realm, target, \"appendChild\", 1, op_append_child::<H>, swb_js::Attributes::MEMBER)?;"));
        let element = file(idl, "element.rs");
        assert!(element.contains(
            "b::reflect::get_string::<H>(&mut b::Cx::new(rt, call, super::ELEMENT), this, \"id\")"
        ));
        assert!(!element.contains("fn id("));
        let module = file(idl, "mod.rs");
        assert!(module.contains(
            "pub const DOM_EXCEPTION: swb_js::ClassId = swb_js::ClassId::from_index(0);"
        ));
        assert!(
            module.contains("pub const ELEMENT: swb_js::ClassId = swb_js::ClassId::from_index(2);")
        );
        assert!(module.contains("swb_js::ClassSpec::new(\"Element\").parent(NODE)"));
        assert!(module.contains("b::glue::expose(rt, realm, \"Node\", interface)?;"));
    }

    #[test]
    fn output_of_types() {
        let idl = "enum Mode { \"a-b\", \"\" }; dictionary D { required long x; Mode m = \"a-b\"; (long or DOMString) u; }; callback F = boolean (any v);";
        let types = file(idl, "types.rs");
        assert!(types.contains(
            "pub enum Mode {\n    /// `\"a-b\"`.\n    AB,\n    /// `\"\"`.\n    Empty,\n}"
        ));
        assert!(types.contains("    pub x: i32,\n"));
        assert!(types.contains("    pub m: Mode,\n"));
        assert!(types.contains("    pub u: Option<LongOrDomString>,\n"));
        assert!(types.contains("return Err(b::convert::required_member(\"x\", \"D\"));"));
        assert!(types.contains("pub enum LongOrDomString {"));
        assert!(types.contains("pub struct F(pub swb_js::Gc<swb_js::Object>);"));
        assert!(types.contains("pub fn invoke<H: b::Host>(self, rt: &mut swb_js::Runtime, _realm: u32, this: swb_js::Value, v: swb_js::Value) -> b::Result<bool> {"));
    }

    #[test]
    fn overloads_dispatch_by_count_and_type() {
        let idl = "[Exposed=Window] interface Node {}; [Exposed=Window] interface T { long f(); long f(Node n); long f(DOMString s); long f(long a, long b, long c); };";
        let t = file(idl, "t.rs");
        assert!(t.contains("match call.argc().min(3) {"));
        assert!(t.contains("2 => Err(b::Exception::type_error(\"Overload resolution failed.\")),"));
        assert!(t.contains("let which = b::overload::select(rt, v, &[b::overload::Candidate { interfaces: &[super::NODE], ..b::overload::Candidate::default() }, b::overload::Candidate { string: true, ..b::overload::Candidate::default() }])?;"));
        assert!(t.contains("fn f_4(cx: &mut b::Cx<'_>, this: b::Platform, a: i32, b: i32, c: i32) -> b::Result<i32>;"));
    }

    #[test]
    fn output_of_indexed_and_named_properties() {
        let idl = "[Exposed=Window] interface Node {};\n[Exposed=Window, LegacyUnenumerableNamedProperties] interface C { getter Node? item(unsigned long index); getter Node (DOMString name); setter undefined (unsigned long index, Node? n); };\n[Exposed=Window] interface D : C {};\n[Exposed=Window] interface L { getter Node? item(unsigned long index); iterable<Node>; };";
        let c = file(idl, "c.rs");
        assert!(c.contains(
            "    fn index_count(cx: &mut b::Cx<'_>, this: b::Platform) -> b::Result<u32>;"
        ));
        assert!(c.contains("    fn supported_names(cx: &mut b::Cx<'_>, this: b::Platform) -> b::Result<Vec<String>>;"));
        assert!(c.contains("    fn is_supported_name(cx: &mut b::Cx<'_>, this: b::Platform, name: &str) -> b::Result<bool> {"));
        assert!(c.contains("    fn named_getter(cx: &mut b::Cx<'_>, this: b::Platform, name: String) -> b::Result<b::NodeId>;"));
        assert!(c.contains("    fn indexed_setter(cx: &mut b::Cx<'_>, this: b::Platform, index: u32, value: Option<b::NodeId>) -> b::Result<()>;"));
        assert!(c.contains("let result = <H as CImpl>::item(&mut b::Cx::without_call(rt, realm, super::C), this, index)?;"));
        assert!(c.contains("unenumerable: true,"));
        assert!(c.contains("prevent_extensions: Some(b::legacy::prevent_extensions),"));
        assert!(
            c.contains("b::iterable::define_indexed_iterator(rt, realm, interface.prototype)?;")
        );
        // The subclass gets hooks that call the parent's trait.
        let d = file(idl, "d.rs");
        assert!(d.contains("<H as super::c::CImpl>::named_getter(&mut b::Cx::without_call(rt, realm, super::C), this, name)?;"));
        assert!(!d.contains("define_indexed_iterator"));
        let module = file(idl, "mod.rs");
        assert!(module.contains("swb_js::ClassSpec::new(\"D\").parent(C).hooks(d::hooks::<H>())"));
        let l = file(idl, "l.rs");
        assert!(l.contains("b::iterable::define_value_iterator(rt, realm, interface.prototype)?;\n    rt.define_method(realm, target, \"item\""));
    }

    #[test]
    fn output_of_a_pair_iterator_and_a_global() {
        let idl = "[Global=Window, Exposed=Window] interface Window {};\n[Exposed=Window] interface H { iterable<DOMString, long>; };";
        let h = file(idl, "h.rs");
        assert!(h.contains("    fn pair_at(cx: &mut b::Cx<'_>, this: b::Platform, index: u32) -> b::Result<Option<(String, i32)>>;"));
        assert!(h.contains("b::iterable::iterator_prototype::<H>(rt, realm, super::H_ITERATOR, iterator_next::<H>)?;"));
        assert!(h.contains("rt.define_method(realm, interface.prototype, \"forEach\", 1, op_for_each::<H>, swb_js::Attributes::MEMBER)?;"));
        assert!(
            h.contains("b::iterable::define_pair_iterator(rt, interface.prototype, entries)?;")
        );
        let module = file(idl, "mod.rs");
        assert!(
            module.contains(
                "pub const H_ITERATOR: swb_js::ClassId = swb_js::ClassId::from_index(3);"
            )
        );
        assert!(module.contains(
            "b::glue::define_class(rt, swb_js::ClassSpec::new(\"H Iterator\"), H_ITERATOR)?;"
        ));
        assert!(module.contains("b::glue::immutable_prototypes(rt, realm, &[WINDOW])?;"));
        assert!(module.contains("pub fn init_global<H: Bindings>("));
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "a table of cases")]
    fn errors_name_the_file_and_line() {
        let cases = [
            (
                "interface A {\n  attribute Missing x;\n};",
                "t.idl:2: unknown type 'Missing'",
            ),
            (
                "interface A : B {};",
                "t.idl:1: 'A' inherits from an unknown interface 'B'",
            ),
            (
                "interface A : B {};\ninterface B : A {};",
                "t.idl:1: the inheritance of 'A' has a cycle",
            ),
            (
                "interface A {\n  [Bogus] attribute long x;\n};",
                "t.idl:2: the extended attribute [Bogus] is not supported on a attribute",
            ),
            (
                "[LegacyFactoryFunction=Image] interface A {};",
                "t.idl:1: [LegacyFactoryFunction] is not generated yet",
            ),
            (
                "interface A { [Reflect] attribute DOMString x; };",
                "t.idl:1: [Reflect] needs an interface that inherits from Node",
            ),
            (
                "interface A { attribute long fooBar; undefined set_fooBar(long x); };",
                "t.idl:1: two members of 'A' map to the Rust name 'set_foo_bar'",
            ),
            (
                "interface A { long f(long a); long f(long b); };",
                "t.idl:1: the overloads of 'f' with 1 arguments cannot be told apart",
            ),
            (
                "interface A {};\ninterface A {};",
                "t.idl:2: 'A' is defined twice",
            ),
            (
                "interface A { undefined f(optional long a = \"x\"); };",
                "t.idl:1: the default value does not fit the type long",
            ),
            (
                "interface A { undefined f(long... a, long b); };",
                "t.idl:1: the variadic argument 'a' is not the last",
            ),
            (
                "interface A { iterable<long>; };",
                "t.idl:1: a value iterator needs an indexed property getter",
            ),
            (
                "interface A {\n getter long (long i);\n};",
                "t.idl:2: the first argument of a special operation must be unsigned long (indexed) or DOMString (named)",
            ),
            (
                "interface A { getter long (unsigned long i, long j); };",
                "t.idl:1: this special operation takes 1 required argument(s)",
            ),
            (
                "interface A { getter long (unsigned long i); deleter undefined (unsigned long i); };",
                "t.idl:1: indexed property deleters do not exist in Web IDL",
            ),
            (
                "interface A { getter long (unsigned long i); getter long item(unsigned long i); };",
                "t.idl:1: 'A' declares this special operation twice",
            ),
            (
                "interface A { setter undefined (DOMString n, long v); };",
                "t.idl:1: a named property setter or deleter needs a named property getter",
            ),
            (
                "interface A { getter long (DOMString n); setter long (DOMString n, long v); };",
                "t.idl:1: a setter must return undefined",
            ),
            (
                "interface A { getter long item(unsigned long i); long item(DOMString s); };",
                "t.idl:1: the special operation 'item' cannot be overloaded",
            ),
            (
                "[LegacyOverrideBuiltIns] interface A { getter long (unsigned long i); };",
                "t.idl:1: [LegacyOverrideBuiltIns] needs a named property getter",
            ),
            (
                "[Global=Window, Exposed=Window] interface Window { getter object (DOMString name); };",
                "t.idl:1: special operations of a [Global] interface (the named properties object) are not generated yet",
            ),
            (
                "interface A { getter long (unsigned long i); iterable<DOMString>; };",
                "t.idl:1: the value type of a value iterator must be the type of the indexed property getter",
            ),
            (
                "interface A { getter long (unsigned long i); iterable<DOMString, long>; };",
                "t.idl:1: a pair iterator cannot be declared on an interface with indexed properties",
            ),
            (
                "interface A { undefined keys(); iterable<DOMString, long>; };",
                "t.idl:1: an iterable interface cannot have a member named 'keys'",
            ),
            (
                "interface A { iterable<DOMString, long>; iterable<long, long>; };",
                "t.idl:1: an interface has at most one iterable declaration",
            ),
            (
                "interface Node {}; interface E : Node { [Reflect] attribute double d; };",
                "t.idl:1: [Reflect] of the type double is not generated",
            ),
            (
                "dictionary D : D {};",
                "t.idl:1: the inheritance of 'D' has a cycle",
            ),
            (
                "dictionary D : E {};\ndictionary E : D {};",
                "t.idl:1: the inheritance of 'D' has a cycle",
            ),
            ("typedef T T;", "t.idl:1: the typedef 'T' refers to itself"),
            (
                "typedef sequence<(long or U)> T;\ntypedef record<DOMString, T> U;",
                "t.idl:1: the typedef 'T' refers to itself",
            ),
        ];
        for (idl, expected) in cases {
            assert_eq!(error(idl), expected, "{idl}");
        }
        let no_dom_exception = generate(
            &[Source {
                name: "x.idl".into(),
                text: "interface A {};".into(),
            }],
            "crate::bindings",
        )
        .unwrap_err();
        assert_eq!(
            no_dom_exception.to_string(),
            "x.idl:1: a set of IDL files needs the interface DOMException (crates/web/idl/webidl.idl)"
        );
    }
}
