//! Modules (ECMA-262 §16.2, <https://tc39.es/ecma262/2025/#sec-modules>):
//! import and export declarations with import attributes, the module
//! early errors (§16.2.1.1, §16.2.2.1, §16.2.3.1) and the module records
//! of `ParseModule` (§16.2.1.7.1). `import()` and `import.meta`, which are
//! expressions, are in [`super::expressions`].
//!
//! Module code is strict, `await` is reserved in it and is an
//! `AwaitExpression` at the top level, and the top-level function
//! declarations are lexical. Imports are lexical bindings of the module
//! scope ([`crate::BindingKind::Import`]).
//!
//! Costs: the requests are deduplicated through a map, the export names
//! through a set, and the exported local names are checked once at the
//! end, so a module with many imports and exports parses in linear time.

use std::collections::{HashMap, HashSet};

use swb_js_text::{CodeUnit, String16};

use super::{IdentUse, PResult, Parser, StmtContext};
use crate::ast::{
    ExprId, FunctionId, PatternId, PatternKind, ScopeId, Span, StmtId, StmtKind, StringId,
};
use crate::error::ParseError;
use crate::interner::{NameId, names};
use crate::messages;
use crate::module::{
    ExportEntry, ExportImportName, ImportAttribute, ImportEntry, ImportName, ModuleRecord,
    ModuleRequest,
};
use crate::scope::BindingKind;
use crate::token::{TokenKind, TokenValue};

/// The state of a module being parsed.
#[derive(Debug, Default)]
pub(crate) struct ModuleState {
    /// The requests and the import entries so far.
    record: ModuleRecord,
    /// The index of each request, by its key (specifier and attributes).
    request_index: HashMap<Vec<u16>, u32>,
    /// `ExportEntries` of the body so far, in source order.
    exports: Vec<ExportEntry>,
    /// The export names so far.
    exported: HashSet<NameId>,
    /// The local names of `export { ... }` without `from`, with their
    /// offsets: each must be declared in the module (checked at the end).
    local_names: Vec<(NameId, u32)>,
}

/// A `ModuleExportName` (§16.2): an identifier name or a string.
#[derive(Clone, Copy, Debug)]
struct ExportName {
    name: NameId,
    offset: u32,
    /// A string literal (cannot name a local binding).
    string: bool,
    /// A reserved word (cannot name a local binding).
    reserved: bool,
}

impl<U: CodeUnit> Parser<'_, '_, U> {
    /// `Module` (§16.2): the module items up to the end; then the module
    /// early errors that need the whole body, the module records and the
    /// scope analysis.
    pub(super) fn parse_module(mut self) -> PResult<crate::Script> {
        self.token = self
            .lexer
            .next_token(crate::token::Goal::HashbangOrRegExp)?;
        // Module code is strict: no directive prologue changes it.
        let body = self.parse_statements(TokenKind::Eof, false)?;
        let state = self.module.take().unwrap_or_default();
        let record = self.finish_module_record(*state)?;
        self.finish(body, Some(record))
    }

    /// Whether the parser is at the top level of a module, where import
    /// and export declarations may appear.
    pub(super) fn at_module_top(&self, context: StmtContext) -> bool {
        self.module.is_some()
            && context == StmtContext::ListItem
            && self.label_chain.is_none()
            && self.ctx.kind == crate::FunctionKind::Module
            && self.scope.index() == 0
    }

    // --- Imports (§16.2.2) ---

    /// `ImportDeclaration`; the current token is `import`.
    pub(super) fn parse_import_declaration(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        self.advance()?;
        if self.at(TokenKind::String) {
            let specifier = self.parse_module_specifier()?;
            let attributes = self.parse_with_clause()?;
            self.add_request(specifier, attributes);
            self.consume_semicolon()?;
            return Ok(self.finish_import(start));
        }
        let mut bindings: Vec<(ImportName, NameId, u32)> = Vec::new();
        // The index of the first named import (`{ a }`).
        let mut named_from = None;
        let mut more = true;
        if self.at_identifier() {
            let (name, offset) = self.parse_imported_binding()?;
            bindings.push((ImportName::Name(names::DEFAULT), name, offset));
            more = self.at(TokenKind::Comma);
            if more {
                let comma = self.token.start;
                self.advance()?;
                if !matches!(self.token.kind, TokenKind::Star | TokenKind::LBrace) {
                    // V8 marks the comma.
                    return Err(ParseError::syntax(comma, messages::unexpected_token(",")));
                }
            }
        }
        if more {
            match self.token.kind {
                TokenKind::Star => {
                    self.advance()?;
                    self.expect_contextual(names::AS)?;
                    let (name, offset) = self.parse_imported_binding()?;
                    bindings.push((ImportName::Namespace, name, offset));
                }
                TokenKind::LBrace => {
                    named_from = Some(bindings.len());
                    self.parse_named_imports(&mut bindings)?;
                }
                _ => return Err(self.unexpected()),
            }
        }
        self.expect_contextual(names::FROM)?;
        let specifier = self.parse_module_specifier()?;
        let attributes = self.parse_with_clause()?;
        let request = self.add_request(specifier, attributes);
        self.consume_semicolon()?;
        let named_from = named_from;
        for (index, (import_name, local_name, offset)) in bindings.into_iter().enumerate() {
            let declared = self.scopes.declare_import(self.scope, local_name, offset);
            // V8 reports a redeclared default or namespace import at the
            // `import`, a named one at its binding.
            let named = named_from.is_some_and(|first| index >= first);
            let at = if named { offset } else { start };
            // No identifier occurrence: the declaration has no code, and the
            // linker initializes the binding ([`ImportEntry::binding`]).
            let binding = self.declared(declared, local_name, at)?;
            if let Some(state) = self.module.as_mut() {
                state.record.import_entries.push(ImportEntry {
                    request,
                    import_name,
                    local_name,
                    binding,
                });
            }
        }
        Ok(self.finish_import(start))
    }

    fn finish_import(&mut self, start: u32) -> StmtId {
        self.ast
            .push_stmt(StmtKind::Import, Span::new(start, self.prev_end))
    }

    /// Consumes the identifier `name` (without escapes), or fails with
    /// "Unexpected token".
    fn expect_contextual(&mut self, name: NameId) -> PResult<()> {
        if self.at_contextual(name) {
            return self.advance();
        }
        if self.at(TokenKind::Identifier) && self.token.escaped && self.token.name() == Some(name) {
            let message = messages::escaped_contextual_keyword(&self.name_text(name));
            return Err(ParseError::syntax(self.token.start, message));
        }
        Err(self.unexpected())
    }

    /// `ImportedBinding` (a `BindingIdentifier` of strict mode code with
    /// `await` reserved); returns the name and its offset. V8 reports a
    /// reserved word here as "Unexpected reserved word".
    fn parse_imported_binding(&mut self) -> PResult<(NameId, u32)> {
        let offset = self.token.start;
        if self.token.kind.is_keyword() && !matches!(self.token.kind, TokenKind::Yield) {
            return Err(ParseError::syntax(offset, messages::UNEXPECTED_RESERVED));
        }
        let name = self.check_identifier(IdentUse::Binding)?;
        self.advance()?;
        Ok((name, offset))
    }

    /// `NamedImports`: `{ a, b as c, "s" as d, }`.
    fn parse_named_imports(
        &mut self,
        bindings: &mut Vec<(ImportName, NameId, u32)>,
    ) -> PResult<()> {
        self.expect(TokenKind::LBrace)?;
        while !self.at(TokenKind::RBrace) {
            let token = self.token.clone();
            let import_name = self.parse_module_export_name()?;
            if self.at_contextual(names::AS) {
                self.advance()?;
                let (local, offset) = self.parse_imported_binding()?;
                bindings.push((ImportName::Name(import_name.name), local, offset));
            } else if import_name.string || import_name.reserved || token.kind.is_keyword() {
                // `{ "s" }`, `{ default }`: V8 reports a reserved word.
                return Err(ParseError::syntax(
                    import_name.offset,
                    messages::UNEXPECTED_RESERVED,
                ));
            } else {
                // `{ a }`: the name is also the binding identifier.
                let local = self.check_identifier_token(&token, IdentUse::Binding)?;
                bindings.push((
                    ImportName::Name(import_name.name),
                    local,
                    import_name.offset,
                ));
            }
            if !self.at(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()
    }

    /// `ModuleSpecifier`: a string literal; returns its value.
    fn parse_module_specifier(&mut self) -> PResult<StringId> {
        let TokenValue::String(value) = &self.token.value else {
            return Err(self.unexpected());
        };
        if self.token.kind != TokenKind::String {
            return Err(self.unexpected());
        }
        let value = value.clone();
        self.advance()?;
        Ok(self.ast.push_string(value))
    }

    /// `WithClause` (§16.2.2): `with { key: "value", ... }`, sorted by key
    /// (`WithClauseToAttributes`). A repeated key is an early error.
    fn parse_with_clause(&mut self) -> PResult<Vec<ImportAttribute>> {
        if !self.at(TokenKind::With) {
            return Ok(Vec::new());
        }
        self.advance()?;
        self.expect(TokenKind::LBrace)?;
        let mut entries: Vec<(String16, ImportAttribute)> = Vec::new();
        let mut keys: HashSet<String16> = HashSet::new();
        while !self.at(TokenKind::RBrace) {
            let key_offset = self.token.start;
            let key = self.attribute_key()?;
            self.advance()?;
            self.expect(TokenKind::Colon)?;
            let TokenValue::String(value) = &self.token.value else {
                return Err(self.unexpected());
            };
            if self.token.kind != TokenKind::String {
                return Err(self.unexpected());
            }
            let value = value.clone();
            self.advance()?;
            if !keys.insert(key.clone()) {
                let message = messages::duplicate_import_attribute(&key.to_string_lossy());
                return Err(ParseError::syntax(key_offset, message));
            }
            let attribute = ImportAttribute {
                key: self.ast.push_string(key.clone()),
                value: self.ast.push_string(value),
            };
            entries.push((key, attribute));
            if !self.at(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()?;
        // Sorted by key as code units (§16.2.2.4).
        entries.sort_by(|a, b| a.0.as_str16().units().cmp(b.0.as_str16().units()));
        Ok(entries
            .into_iter()
            .map(|(_, attribute)| attribute)
            .collect())
    }

    /// The text of an `AttributeKey` (an identifier name or a string).
    fn attribute_key(&self) -> PResult<String16> {
        let name = match (&self.token.kind, &self.token.value) {
            (TokenKind::String, TokenValue::String(value)) => return Ok(value.clone()),
            (TokenKind::Identifier, _) => self.token.name(),
            (kind, _) => kind.keyword_name(),
        };
        match name {
            Some(name) => Ok(self
                .lexer
                .interner()
                .get(name)
                .map(swb_js_text::Str16::to_string16)
                .unwrap_or_default()),
            None => Err(self.unexpected()),
        }
    }

    /// Adds a module request, once per specifier and attributes
    /// (`ModuleRequests`, §16.2.1.4); returns its index.
    fn add_request(&mut self, specifier: StringId, attributes: Vec<ImportAttribute>) -> u32 {
        let mut key: Vec<u16> = self.ast.string(specifier).as_str16().units().collect();
        for attribute in &attributes {
            for id in [attribute.key, attribute.value] {
                key.push(0xFFFF);
                key.extend(self.ast.string(id).as_str16().units());
            }
        }
        let Some(state) = self.module.as_mut() else {
            return 0;
        };
        if let Some(&index) = state.request_index.get(&key) {
            return index;
        }
        let index = state.record.requests.len() as u32;
        state.record.requests.push(ModuleRequest {
            specifier,
            attributes,
        });
        state.request_index.insert(key, index);
        index
    }

    // --- Exports (§16.2.3) ---

    /// `ExportDeclaration`; the current token is `export`.
    pub(super) fn parse_export_declaration(&mut self) -> PResult<StmtId> {
        let start = self.token.start;
        let export_token = self.token.clone();
        self.advance()?;
        let declaration = self.at_let_or_async_declaration()?;
        let kind = match self.token.kind {
            TokenKind::Star => self.parse_export_star()?,
            TokenKind::LBrace => self.parse_export_names()?,
            TokenKind::Default => return self.parse_export_default(start),
            TokenKind::Var | TokenKind::Const | TokenKind::Function | TokenKind::Class => {
                self.parse_export_declared(start)?
            }
            TokenKind::Identifier if declaration => self.parse_export_declared(start)?,
            // `async` without `function` on the same line: V8 reports the
            // `async` token as a token, not as an identifier.
            _ if self.at_contextual(names::ASYNC) => {
                return Err(ParseError::syntax(
                    self.token.start,
                    messages::unexpected_token("async"),
                ));
            }
            // V8 reports the `export` token.
            _ => return Err(self.unexpected_token(&export_token)),
        };
        Ok(self.ast.push_stmt(kind, Span::new(start, self.prev_end)))
    }

    /// Whether the current token starts `let` or `async function`
    /// declaration.
    fn at_let_or_async_declaration(&mut self) -> PResult<bool> {
        if self.at_let_declaration()? {
            return Ok(true);
        }
        if !self.at_contextual(names::ASYNC) {
            return Ok(false);
        }
        let next = self.peek()?;
        Ok(next.kind == TokenKind::Function && !next.newline_before)
    }

    /// `export * from "m";` and `export * as name from "m";`.
    fn parse_export_star(&mut self) -> PResult<StmtKind> {
        self.advance()?;
        let mut export_name = None;
        let mut name_offset = 0;
        if self.at_contextual(names::AS) {
            self.advance()?;
            let name = self.parse_module_export_name()?;
            export_name = Some(name.name);
            name_offset = name.offset;
        }
        self.expect_contextual(names::FROM)?;
        let specifier = self.parse_module_specifier()?;
        let attributes = self.parse_with_clause()?;
        let request = self.add_request(specifier, attributes);
        self.consume_semicolon()?;
        if let Some(name) = export_name {
            self.add_export_name(name, name_offset)?;
        }
        let import_name = if export_name.is_some() {
            ExportImportName::All
        } else {
            ExportImportName::AllButDefault
        };
        self.push_export(ExportEntry {
            export_name,
            request: Some(request),
            import_name: Some(import_name),
            local_name: None,
        });
        Ok(StmtKind::ExportList)
    }

    /// `export { a, b as c };` or `export { a, "b" as c } from "m";`.
    fn parse_export_names(&mut self) -> PResult<StmtKind> {
        self.expect(TokenKind::LBrace)?;
        let mut specifiers: Vec<(ExportName, ExportName)> = Vec::new();
        while !self.at(TokenKind::RBrace) {
            let local = self.parse_module_export_name()?;
            let exported = if self.at_contextual(names::AS) {
                self.advance()?;
                self.parse_module_export_name()?
            } else {
                local
            };
            specifiers.push((local, exported));
            if !self.at(TokenKind::RBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.advance()?;
        if self.at_contextual(names::FROM) {
            self.advance()?;
            let specifier = self.parse_module_specifier()?;
            let attributes = self.parse_with_clause()?;
            let request = self.add_request(specifier, attributes);
            self.consume_semicolon()?;
            self.add_export_names(&specifiers)?;
            for (local, exported) in specifiers {
                self.push_export(ExportEntry {
                    export_name: Some(exported.name),
                    request: Some(request),
                    import_name: Some(ExportImportName::Name(local.name)),
                    local_name: None,
                });
            }
            return Ok(StmtKind::ExportList);
        }
        // The local names are references (§16.2.3.1).
        for (local, _) in &specifiers {
            if local.string {
                return Err(ParseError::syntax(
                    local.offset,
                    messages::STRING_EXPORT_WITHOUT_FROM,
                ));
            }
            if local.reserved || local.name == names::AWAIT {
                return Err(ParseError::syntax(
                    local.offset,
                    messages::UNEXPECTED_RESERVED,
                ));
            }
        }
        self.consume_semicolon()?;
        self.add_export_names(&specifiers)?;
        for (local, exported) in specifiers {
            if let Some(state) = self.module.as_mut() {
                state.local_names.push((local.name, local.offset));
            }
            self.push_export(ExportEntry {
                export_name: Some(exported.name),
                request: None,
                import_name: None,
                local_name: Some(local.name),
            });
        }
        Ok(StmtKind::ExportList)
    }

    /// Records the export names of a list (V8 checks them when the
    /// statement has ended; it reports a repeated name at its specifier).
    fn add_export_names(&mut self, specifiers: &[(ExportName, ExportName)]) -> PResult<()> {
        for (local, exported) in specifiers {
            self.add_export_name(exported.name, local.offset)?;
        }
        Ok(())
    }

    /// `ModuleExportName`: an identifier name (also a reserved word) or a
    /// string literal without lone surrogates (§16.2.1.1).
    fn parse_module_export_name(&mut self) -> PResult<ExportName> {
        let offset = self.token.start;
        let (name, string, reserved) = match (&self.token.kind, &self.token.value) {
            (TokenKind::String, TokenValue::String(value)) => {
                if !is_well_formed(value) {
                    return Err(ParseError::syntax(
                        offset,
                        messages::UNPAIRED_SURROGATE_EXPORT_NAME,
                    ));
                }
                let value = value.clone();
                (
                    self.lexer.interner_mut().intern(value.as_str16()),
                    true,
                    false,
                )
            }
            (TokenKind::Identifier, _) => {
                let name = self.token.name().unwrap_or(names::AWAIT);
                let reserved =
                    TokenKind::keyword_from_name(name).is_some() || super::is_strict_reserved(name);
                (name, false, reserved)
            }
            (kind, _) => match kind.keyword_name() {
                Some(name) => (name, false, *kind != TokenKind::Await),
                None => return Err(self.unexpected()),
            },
        };
        self.advance()?;
        Ok(ExportName {
            name,
            offset,
            string,
            reserved,
        })
    }

    /// `export` with a variable, function or class declaration.
    fn parse_export_declared(&mut self, start: u32) -> PResult<StmtKind> {
        let declaration_start = self.token.start;
        let declaration = self.parse_statement(StmtContext::ListItem)?;
        let mut bound = Vec::new();
        self.declared_names(declaration, &mut bound);
        for name in bound {
            self.add_export_name(name, declaration_start)?;
            self.push_export(ExportEntry {
                export_name: Some(name),
                request: None,
                import_name: None,
                local_name: Some(name),
            });
            if let Some(state) = self.module.as_mut() {
                state.local_names.push((name, start));
            }
        }
        Ok(StmtKind::ExportDeclaration(declaration))
    }

    /// The names that a declaration statement binds (`BoundNames`).
    fn declared_names(&self, stmt: StmtId, out: &mut Vec<NameId>) {
        match self.ast.stmt(stmt).kind {
            StmtKind::Variables { declarators, .. } => {
                let targets: Vec<PatternId> = self
                    .ast
                    .declarators(declarators)
                    .iter()
                    .map(|d| d.target)
                    .collect();
                self.pattern_names(targets, out);
            }
            StmtKind::Function(function) => out.extend(self.function_name(function)),
            StmtKind::Class(class) => out.extend(self.ast.class(class).name),
            _ => {}
        }
    }

    fn function_name(&self, function: FunctionId) -> Option<NameId> {
        self.ast.function(function).name.map(|ident| ident.name)
    }

    /// The binding names of patterns, without recursion.
    fn pattern_names(&self, mut stack: Vec<PatternId>, out: &mut Vec<NameId>) {
        let mut found = Vec::new();
        while let Some(id) = stack.pop() {
            match self.ast.pattern(id).kind {
                PatternKind::Identifier(ident) => found.push((id, ident.name)),
                PatternKind::Array(list) => stack.extend(self.ast.patterns(list).iter().rev()),
                PatternKind::Object { properties, rest } => {
                    stack.extend(rest);
                    let values = self.ast.pattern_properties(properties);
                    stack.extend(values.iter().rev().map(|p| p.value));
                }
                PatternKind::Default { target, .. } | PatternKind::Rest(target) => {
                    stack.push(target);
                }
                PatternKind::Expr(_) | PatternKind::Hole => {}
            }
        }
        // Source order (pattern ids grow in source order of the leaves).
        found.sort_by_key(|&(id, _)| id);
        out.extend(found.into_iter().map(|(_, name)| name));
    }

    /// `export default` with a function, class or expression (§16.2.3).
    /// Without a name, the declaration binds `*default*`; V8 reports a
    /// second one as a redeclaration of its `.default` binding, a second
    /// default export of another form as a duplicate export.
    fn parse_export_default(&mut self, start: u32) -> PResult<StmtId> {
        let default_offset = self.token.start;
        self.advance()?;
        let declaration_start = self.token.start;
        let is_async = self.at_async_function_start()?;
        let local = match self.token.kind {
            TokenKind::Function => {
                let function = self.parse_function_declaration_default(declaration_start, false)?;
                self.function_declaration_stmt(function, declaration_start)
            }
            TokenKind::Identifier if is_async => {
                self.advance()?;
                let function = self.parse_function_declaration_default(declaration_start, true)?;
                self.function_declaration_stmt(function, declaration_start)
            }
            TokenKind::Class => {
                let class = self.parse_class_default()?;
                let stmt = self.ast.push_stmt(
                    StmtKind::Class(class),
                    Span::new(declaration_start, self.prev_end),
                );
                (stmt, self.ast.class(class).name)
            }
            _ => return self.parse_export_default_expression(start, default_offset),
        };
        let (declaration, name) = local;
        self.add_default_export(name.unwrap_or(names::DEFAULT_EXPORT), default_offset)?;
        Ok(self.ast.push_stmt(
            StmtKind::ExportDeclaration(declaration),
            Span::new(start, self.prev_end),
        ))
    }

    /// Whether the current token is `async` followed by `function` on the
    /// same line.
    fn at_async_function_start(&mut self) -> PResult<bool> {
        if !self.at_contextual(names::ASYNC) {
            return Ok(false);
        }
        let next = self.peek()?;
        Ok(next.kind == TokenKind::Function && !next.newline_before)
    }

    fn function_declaration_stmt(
        &mut self,
        function: FunctionId,
        start: u32,
    ) -> (StmtId, Option<NameId>) {
        let stmt = self.ast.push_stmt(
            StmtKind::Function(function),
            Span::new(start, self.prev_end),
        );
        (stmt, self.function_name(function))
    }

    /// `export default AssignmentExpression;`: the value initializes the
    /// binding `*default*`.
    fn parse_export_default_expression(
        &mut self,
        start: u32,
        default_offset: u32,
    ) -> PResult<StmtId> {
        let value: ExprId = self.parse_assignment(false)?;
        self.consume_semicolon()?;
        let offset = self.ast.expr(value).span.start;
        let declared = self.scopes.declare_lexical(
            self.scope,
            names::DEFAULT_EXPORT,
            BindingKind::Let,
            offset,
        );
        self.declared(declared, names::DEFAULT_EXPORT, default_offset)?;
        let binding = self
            .reference(names::DEFAULT_EXPORT, offset, true)
            .reference;
        self.add_default_export(names::DEFAULT_EXPORT, default_offset)?;
        Ok(self.ast.push_stmt(
            StmtKind::ExportDefault { value, binding },
            Span::new(start, self.prev_end),
        ))
    }

    /// Records the export entry of `export default` with its local name.
    fn add_default_export(&mut self, local_name: NameId, offset: u32) -> PResult<()> {
        self.add_export_name(names::DEFAULT, offset)?;
        if let Some(state) = self.module.as_mut() {
            state.local_names.push((local_name, offset));
        }
        self.push_export(ExportEntry {
            export_name: Some(names::DEFAULT),
            request: None,
            import_name: None,
            local_name: Some(local_name),
        });
        Ok(())
    }

    /// Records an export name; a repeated one is an early error
    /// (§16.2.1.1), which V8 reports at `offset`.
    fn add_export_name(&mut self, name: NameId, offset: u32) -> PResult<()> {
        let Some(state) = self.module.as_mut() else {
            return Ok(());
        };
        if state.exported.insert(name) {
            return Ok(());
        }
        Err(ParseError::syntax(
            offset,
            messages::duplicate_export(&self.name_text(name)),
        ))
    }

    fn push_export(&mut self, entry: ExportEntry) {
        if let Some(state) = self.module.as_mut() {
            state.exports.push(entry);
        }
    }

    /// The end of a module: every exported local name must be declared
    /// (§16.2.1.1); the exported bindings become cells; the export entries
    /// are sorted into local, indirect and star entries (§16.2.1.7.1).
    fn finish_module_record(&mut self, state: ModuleState) -> PResult<ModuleRecord> {
        let scope = ScopeId::from_index(0);
        for &(name, offset) in &state.local_names {
            if !self.scopes.mark_exported(scope, name) {
                let message = messages::undefined_export(&self.name_text(name));
                return Err(ParseError::syntax(offset, message));
            }
        }
        let mut record = state.record;
        let imports: HashMap<NameId, ImportEntry> = record
            .import_entries
            .iter()
            .map(|entry| (entry.local_name, *entry))
            .collect();
        for entry in state.exports {
            split_export(&mut record, &imports, entry);
        }
        record.has_top_level_await = self.module_await;
        Ok(record)
    }
}

/// Sorts one export entry into the records (§16.2.1.7.1 step 10).
fn split_export(
    record: &mut ModuleRecord,
    imports: &HashMap<NameId, ImportEntry>,
    entry: ExportEntry,
) {
    if entry.request.is_some() {
        if entry.import_name == Some(ExportImportName::AllButDefault) {
            record.star_exports.push(entry);
        } else {
            record.indirect_exports.push(entry);
        }
        return;
    }
    let import = entry.local_name.and_then(|name| imports.get(&name));
    match import {
        Some(import) => match import.import_name {
            // A re-export of an imported namespace object.
            ImportName::Namespace => record.local_exports.push(entry),
            ImportName::Name(name) => record.indirect_exports.push(ExportEntry {
                export_name: entry.export_name,
                request: Some(import.request),
                import_name: Some(ExportImportName::Name(name)),
                local_name: None,
            }),
        },
        None => record.local_exports.push(entry),
    }
}

/// Whether a string has no lone surrogates (`IsStringWellFormedUnicode`).
fn is_well_formed(value: &String16) -> bool {
    let mut units = value.as_str16().units().peekable();
    while let Some(unit) = units.next() {
        match unit {
            0xD800..=0xDBFF => {
                if units.next_if(|u| (0xDC00..=0xDFFF).contains(u)).is_none() {
                    return false;
                }
            }
            0xDC00..=0xDFFF => return false,
            _ => {}
        }
    }
    true
}
