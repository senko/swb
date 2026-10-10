//! The module records of a parsed module (§16.2.1.7.1 `ParseModule`,
//! <https://tc39.es/ecma262/2025/#sec-parsemodule>): the requested modules
//! with their import attributes, the import entries and the local,
//! indirect and star export entries. Loading and linking come with M7
//! feature 14 (ADR 0026).
//!
//! Contract for the linker and the compiler:
//!
//! - The top-level declarations of a module are bindings of its
//!   [`crate::ScopeKind::Module`] scope: registers or cells of the module
//!   function, not globals. A declared binding that a local export entry
//!   names is always a cell ([`crate::Binding::captured`]), so that the
//!   linker can give the same cell to the importing modules. A local
//!   export can also name a namespace import (`import * as ns from "m";
//!   export { ns }`, §16.2.1.7.1 step 10.a.ii.2): the binding is an import
//!   and has no own cell; the linker exports the cell that it gives the
//!   module for that import entry. `export default` of an
//!   expression, or of an anonymous function or class, binds the local
//!   name `*default*` (not an identifier, so no code can name it).
//! - An import binding ([`crate::BindingKind::Import`]) has no storage in
//!   the module's frame. The module function receives the cell of each
//!   import entry as a capture whose source is
//!   [`crate::CaptureSource::Import`] with the index of the entry in
//!   [`ModuleRecord::import_entries`]; for a namespace import the cell
//!   holds the namespace object. Functions in the module capture it from
//!   there like any other cell. Loads check the temporal dead zone,
//!   because the exporting module may not have initialized the cell yet.
//! - Indirect and star export entries are resolved by the linker
//!   (`ResolveExport`, §16.2.1.7.2.2); they have no binding in the module.

use crate::ast::{BindingId, StringId};
use crate::interner::NameId;

/// The module records of a module.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModuleRecord {
    /// `[[RequestedModules]]`: the module requests in source order, each
    /// once (`ModuleRequestsEqual`, §16.2.1.3.1).
    pub requests: Vec<ModuleRequest>,
    /// `[[ImportEntries]]`, in source order.
    pub import_entries: Vec<ImportEntry>,
    /// `[[LocalExportEntries]]`.
    pub local_exports: Vec<ExportEntry>,
    /// `[[IndirectExportEntries]]`.
    pub indirect_exports: Vec<ExportEntry>,
    /// `[[StarExportEntries]]`.
    pub star_exports: Vec<ExportEntry>,
    /// `[[HasTLA]]`: the module body contains `await` (outside functions).
    pub has_top_level_await: bool,
}

/// A module request (§16.2.1.3): a specifier with its import attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRequest {
    /// The specifier (the string value; in the script's string table).
    pub specifier: StringId,
    /// The import attributes, sorted by key (`WithClauseToAttributes`,
    /// §16.2.2.4).
    pub attributes: Vec<ImportAttribute>,
}

/// An import attribute (`with { type: "json" }`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportAttribute {
    /// The key.
    pub key: StringId,
    /// The value.
    pub value: StringId,
}

/// What an import entry imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportName {
    /// An export name of the requested module.
    Name(NameId),
    /// The namespace object (`import * as ns`).
    Namespace,
}

/// An import entry (Table 61).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportEntry {
    /// The request: an index in [`ModuleRecord::requests`].
    pub request: u32,
    /// The imported name.
    pub import_name: ImportName,
    /// The local name.
    pub local_name: NameId,
    /// The local binding ([`crate::BindingKind::Import`]).
    pub binding: BindingId,
}

/// What an export entry re-exports from another module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportImportName {
    /// An export name of the requested module.
    Name(NameId),
    /// `all`: `export * as ns from "m"`.
    All,
    /// `all-but-default`: `export * from "m"`.
    AllButDefault,
}

/// An export entry (Table 62).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportEntry {
    /// The exported name; `None` for `export * from "m"`.
    pub export_name: Option<NameId>,
    /// The request: an index in [`ModuleRecord::requests`]; `None` for a
    /// local export.
    pub request: Option<u32>,
    /// The imported name for a re-export; `None` for a local export.
    pub import_name: Option<ExportImportName>,
    /// The local name of a local export.
    pub local_name: Option<NameId>,
}
