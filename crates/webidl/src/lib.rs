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
        // Without a `[Global]` interface every interface is lazy: a global
        // property in the order of the classes, and a dispatch from the
        // class to its `define`.
        assert!(module.contains("b::glue::expose_lazy(rt, realm, GLOBALS, 1)?; // Node"));
        assert!(module.contains("        NODE => node::define::<H>(rt, realm)?,"));
        assert!(module.contains("rt.set_lazy_hooks(swb_js::LazyHooks { interface: create_interface::<H>, global: global_value::<H> });"));
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
    fn unions_with_undefined_and_inherited_unforgeables() {
        let idl = "[Exposed=Window] interface E { [LegacyUnforgeable] readonly attribute boolean isTrusted; };\n[Exposed=Window] interface C : E {};\n[Exposed=Window] interface W { readonly attribute (E or undefined) event; };";
        let types = file(idl, "types.rs");
        assert!(
            types.contains("if value.is_undefined() { return Ok(EOrUndefined::Undefined(())); }")
        );
        assert!(types.contains(
            "EOrUndefined::Undefined(()) => swb_js::VmResult::Ok(swb_js::Value::Undefined),"
        ));
        let e = file(idl, "e.rs");
        assert!(e.contains("    let template = b::glue::new_unforgeable_template::<H>"));
        // A subclass without unforgeable members of its own copies the
        // parent's template only.
        let c = file(idl, "c.rs");
        assert!(c.contains("    let _template = b::glue::new_unforgeable_template::<H>(rt, realm, super::C, Some(super::E))?;"));
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
        assert!(c.contains(
            "b::iterable::define_indexed_iterator::<H>(rt, realm, interface.prototype)?;"
        ));
        // The subclass gets hooks that call the parent's trait.
        let d = file(idl, "d.rs");
        assert!(d.contains("<H as super::c::CImpl>::named_getter(&mut b::Cx::without_call(rt, realm, super::C), this, name)?;"));
        assert!(!d.contains("define_indexed_iterator"));
        let module = file(idl, "mod.rs");
        assert!(module.contains("swb_js::ClassSpec::new(\"D\").parent(C).hooks(d::hooks::<H>())"));
        let l = file(idl, "l.rs");
        assert!(l.contains("b::iterable::define_value_iterator::<H>(rt, realm, interface.prototype)?;\n    rt.define_method(realm, target, \"item\""));
    }

    #[test]
    fn an_html_constructor_throws_illegal_constructor() {
        let idl = "[Exposed=Window] interface E { [HTMLConstructor] constructor(); };";
        let e = file(idl, "e.rs");
        assert!(e.contains("pub(super) fn construct<H: EImpl>("));
        assert!(e.contains("b::Context::Constructor(\"E\")"));
        assert!(e.contains("Err(b::Exception::type_error(\"Illegal constructor\"))"));
        // The trait has no constructor method to write.
        assert!(!e.contains("fn constructor("));
        // `new E()` is the only member, so the trait is empty and ready.
        assert!(e.contains("impl<T: b::Host> EImpl for T {}"));
    }

    #[test]
    fn a_legacy_factory_function_is_a_constructor_of_the_global_object() {
        let idl = "[Exposed=Window] interface Node {};\n[Exposed=Window] interface Element : Node {};\n[Exposed=Window, LegacyFactoryFunction=Img(optional unsigned long width, optional unsigned long height), LegacyFactoryFunction=Opt(DOMString text, optional boolean sel = false)]\ninterface Img : Element { [HTMLConstructor] constructor(); };";
        let img = file(idl, "img.rs");
        // The trait has one method per factory, with the arguments of the
        // attribute (an optional argument without a default is an Option).
        assert!(img.contains("fn legacy_factory_img(cx: &mut b::Cx<'_>, width: Option<u32>, height: Option<u32>) -> b::Result<swb_js::Gc<swb_js::Object>>;"));
        assert!(img.contains("fn legacy_factory_opt(cx: &mut b::Cx<'_>, text: String, sel: bool) -> b::Result<swb_js::Gc<swb_js::Object>>;"));
        // The glue is a constructor named by the factory, and a call
        // without `new` throws Chromium's message with the factory's name.
        assert!(img.contains("fn construct_img<H: ImgImpl>("));
        assert!(img.contains("b::Context::Constructor(\"Img\")"));
        assert!(img.contains("if call.new_target().is_undefined() {"));
        assert!(img.contains("Please use the 'new' operator, this DOM object constructor cannot be called as a function."));
        assert!(img.contains(
            "<H as ImgImpl>::legacy_factory_img(&mut b::Cx::new(rt, call, super::IMG), a0, a1)"
        ));
        // The HTML constructor of the interface itself is unchanged.
        assert!(img.contains("pub(super) fn construct<H: ImgImpl>("));
        assert!(img.contains("Err(b::Exception::type_error(\"Illegal constructor\"))"));
        // `define` makes the functions with the prototype of the
        // interface and a `length` of the required arguments (0 and 1),
        // after `constructor` of the prototype, and keeps them for the
        // global object.
        assert!(img.contains("rt.end_interface(interface)?;\n    let factory = rt.define_constructor(realm, interface.prototype, \"Img\", 0, construct_img::<H>)?;\n    b::glue::keep_factory::<H>(rt, realm, \"Img\", factory)?;"));
        assert!(img.contains(
            "rt.define_constructor(realm, interface.prototype, \"Opt\", 1, construct_opt::<H>)?;"
        ));
        // The global object gets them as lazy properties, before the
        // interface object (the order of the eager creation).
        let module = file(idl, "mod.rs");
        assert!(module.contains("    b::glue::GlobalName { name: \"Img\", class: IMG, factory: true },\n    b::glue::GlobalName { name: \"Opt\", class: IMG, factory: true },\n    b::glue::GlobalName { name: \"Img\", class: IMG, factory: false },"));
        assert!(module.contains("    b::glue::expose_lazy(rt, realm, GLOBALS, 3)?; // Img\n    b::glue::expose_lazy(rt, realm, GLOBALS, 4)?; // Opt\n    b::glue::expose_lazy(rt, realm, GLOBALS, 5)?; // Img\n"));
        // An interface that is not exposed in `Window` gets no function.
        let hidden = file(
            "[Exposed=Worker, LegacyFactoryFunction=W()] interface W {};",
            "w.rs",
        );
        assert!(hidden.contains("fn legacy_factory_w("));
        assert!(!hidden.contains("define_constructor"));
    }

    #[test]
    fn a_legacy_window_alias_is_another_global_property_of_the_interface() {
        let idl = "[Exposed=Window] interface Plain {};\n[Exposed=Window, LegacyWindowAlias=(OldA, OldB)] interface Matrix { undefined set(); };\n[Exposed=Window] interface After {};";
        let module = file(idl, "mod.rs");
        // The aliases follow the interface object in the list of globals,
        // with the class of the interface and no factory.
        assert!(module.contains("    b::glue::GlobalName { name: \"Matrix\", class: MATRIX, factory: false },\n    b::glue::GlobalName { name: \"OldA\", class: MATRIX, factory: false },\n    b::glue::GlobalName { name: \"OldB\", class: MATRIX, factory: false },\n    b::glue::GlobalName { name: \"After\", class: AFTER, factory: false },"));
        // The lazy properties are numbered in the same order.
        assert!(module.contains("    b::glue::expose_lazy(rt, realm, GLOBALS, 2)?; // Matrix\n    b::glue::expose_lazy(rt, realm, GLOBALS, 3)?; // OldA\n    b::glue::expose_lazy(rt, realm, GLOBALS, 4)?; // OldB\n    b::glue::expose_lazy(rt, realm, GLOBALS, 5)?; // After\n"));
    }

    #[test]
    fn late_members_follow_the_constructor() {
        let idl = "[Exposed=Window, SwbLateMembers=(zed, toString)] interface Late { constructor(); undefined alpha(); undefined zed(); undefined beta(); stringifier; };";
        let late = file(idl, "late.rs");
        let order = |needle: &str| late.find(needle).unwrap_or(usize::MAX);
        assert!(order("\"alpha\"") < order("\"beta\""));
        assert!(order("\"beta\"") < order("rt.end_interface(interface)?;"));
        assert!(order("rt.end_interface(interface)?;") < order("target, \"toString\""));
        assert!(order("rt.end_interface(interface)?;") < order("target, \"zed\""));
        // A name that is not an operation is an error, not ignored.
        let typo =
            error("[Exposed=Window, SwbLateMembers=zad] interface Late { undefined zed(); };");
        assert!(typo.contains("`zad`, which is not an operation"), "{typo}");
    }

    #[test]
    fn event_handler_attributes_call_the_host() {
        let idl = "[Exposed=Window] interface Node {};\n[Exposed=Window] interface Element : Node {};\ncallback EventHandlerNonNull = any (any event);\ntypedef EventHandlerNonNull? EventHandler;\ninterface mixin GlobalEventHandlers { attribute EventHandler onclick; };\n[Exposed=Window] interface HTMLElement : Element {};\nHTMLElement includes GlobalEventHandlers;\n[Global=Window, Exposed=Window] interface Window {};\nWindow includes GlobalEventHandlers;";
        let element = file(idl, "html_element.rs");
        // No trait method, a getter and a setter that name the target and
        // the attribute.
        assert!(!element.contains("fn onclick("));
        assert!(element.contains(
            "<H as b::Host>::event_handler(rt, b::HandlerTarget::Node(this), \"onclick\")?"
        ));
        assert!(element.contains(
            "<H as b::Host>::set_event_handler(rt, b::HandlerTarget::Node(this), \"onclick\", v)?;"
        ));
        assert!(element.contains("rt.define_accessor(realm, target, \"onclick\", (Some(get_onclick::<H> as swb_js::NativeFn), Some(set_onclick::<H> as swb_js::NativeFn)), swb_js::Attributes::MEMBER)?;"));
        // The window is the target of its own attributes.
        let window = file(idl, "window.rs");
        assert!(window.contains("b::HandlerTarget::Window, \"onclick\""));
        // Another object is the target of its own attributes.
        let other = file(
            "callback EventHandlerNonNull = any (any event);\ntypedef EventHandlerNonNull? EventHandler;\n[Exposed=Window] interface Q { attribute EventHandler onchange; };",
            "q.rs",
        );
        assert!(other.contains("b::HandlerTarget::Object(this.object), \"onchange\""));
    }

    #[test]
    fn rejects_on_error_turns_exceptions_into_promises() {
        let idl = "[Exposed=Window] interface R { [SwbRejectsOnError] undefined f(long a); undefined g(long a); };";
        let r = file(idl, "r.rs");
        assert!(r.contains("fn op_f<H: RImpl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {\n    b::glue::run_promise("));
        assert!(r.contains("fn op_g<H: RImpl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {\n    b::glue::run("));
    }

    #[test]
    fn reflect_setter_covers_strings_and_integers() {
        let idl = "[Exposed=Window] interface Node {};\n[Exposed=Window] interface Element : Node { [ReflectSetter] attribute long a; [ReflectSetter] attribute unsigned long b; [ReflectSetter] attribute DOMString c; attribute long d; };";
        let element = file(idl, "element.rs");
        assert!(element.contains("b::reflect::set_long::<H>(&mut b::Cx::new(rt, call, super::ELEMENT), this, \"a\", false, value)"));
        assert!(element.contains("b::reflect::set_unsigned_long::<H>(&mut b::Cx::new(rt, call, super::ELEMENT), this, \"b\", b::reflect::UnsignedLimit::None, None, value)"));
        assert!(element.contains(
            "b::reflect::set_string::<H>(&mut b::Cx::new(rt, call, super::ELEMENT), this, \"c\", &value)"
        ));
        // The getters are written by hand, the setters are generated; a
        // plain attribute has both in the trait.
        assert!(element.contains("fn a(cx: &mut b::Cx<'_>, this: b::NodeId) -> b::Result<i32>;"));
        assert!(!element.contains("fn set_a("));
        assert!(!element.contains("fn set_b("));
        assert!(!element.contains("fn set_c("));
        assert!(element.contains("fn set_d("));
        // Other types are an error.
        let bad = "[Exposed=Window] interface Node {};\n[Exposed=Window] interface Element : Node { [ReflectSetter] attribute boolean x; };";
        assert!(
            error(bad).contains("[ReflectSetter] needs a string, long or unsigned long attribute")
        );
    }

    #[test]
    fn enumerable_named_properties_override_the_inherited_flag() {
        let idl = "[Exposed=Window] interface Node {};\n[Exposed=Window, LegacyUnenumerableNamedProperties] interface C { getter Node? item(unsigned long index); getter Node (DOMString name); };\n[Exposed=Window, SwbEnumerableNamedProperties] interface D : C {};\n[Exposed=Window] interface E : D {};\n[Exposed=Window] interface F : C {};";
        assert!(file(idl, "c.rs").contains("unenumerable: true,"));
        // The derived-most declaration decides.
        assert!(file(idl, "d.rs").contains("unenumerable: false,"));
        assert!(file(idl, "e.rs").contains("unenumerable: false,"));
        assert!(file(idl, "f.rs").contains("unenumerable: true,"));
    }

    #[test]
    fn static_attributes_buffer_types_and_frozen_arrays() {
        let idl = "[Exposed=Window] interface S {\n  static readonly attribute FrozenArray<DOMString> types;\n  readonly attribute FrozenArray<DOMString> languages;\n  undefined fill(ArrayBufferView array, [AllowShared] Uint8Array bytes, optional AllowSharedBufferSource source);\n};";
        let s = file(idl, "s.rs");
        // A static attribute has no `this`; its accessor is on the
        // interface object.
        assert!(s.contains("    fn types(cx: &mut b::Cx<'_>) -> b::Result<Vec<String>>;"));
        assert!(s.contains(
            "rt.define_accessor(realm, interface.object, \"types\", (Some(get_types::<H> as swb_js::NativeFn), None), swb_js::Attributes::MEMBER)?;"
        ));
        assert!(s.contains("b::to_js::frozen_sequence(rt,"));
        // The buffer source types are objects with a check that names them.
        assert!(s.contains("array: swb_js::Gc<swb_js::Object>"));
        assert!(s.contains("b::convert::buffer(rt, v, \"ArrayBufferView\")"));
        assert!(s.contains("b::convert::buffer(rt, v, \"Uint8Array\")"));
        assert!(s.contains("b::convert::buffer(rt, v, \"AllowSharedBufferSource\")"));
        // A writable static attribute is not generated.
        assert!(
            error("[Exposed=Window] interface S { static attribute long x; };")
                .contains("only readonly static attributes are generated")
        );
    }

    #[test]
    fn a_trait_without_items_has_a_blanket_implementation() {
        let idl = "[Exposed=Window] interface Empty {};\n[Exposed=Window] interface Full { readonly attribute long x; };";
        assert!(file(idl, "empty.rs").contains("impl<T: b::Host> EmptyImpl for T {}"));
        assert!(!file(idl, "full.rs").contains("impl<T: b::Host> FullImpl for T {}"));
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
        // The `[Global]` interface is created with the runtime.
        assert!(module.contains("    let interface = window::define::<H>(rt, realm)?;\n    b::glue::expose(rt, realm, \"Window\", interface)?;"));
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
                "t.idl:1: [LegacyFactoryFunction] needs a name and arguments: [LegacyFactoryFunction=Name(args)]",
            ),
            (
                "[LegacyFactoryFunction(optional long w)] interface A {};",
                "t.idl:1: [LegacyFactoryFunction] needs a name and arguments: [LegacyFactoryFunction=Name(args)]",
            ),
            (
                "[LegacyFactoryFunction=I(Missing m)] interface A {};",
                "t.idl:1: unknown type 'Missing'",
            ),
            (
                "interface A { [LegacyFactoryFunction=I()] attribute long x; };",
                "t.idl:1: the extended attribute [LegacyFactoryFunction] is not supported on a attribute",
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
