//! Readable dumps of a parsed script, for tests and debugging: the tree
//! as S-expressions, and the results of the scope analysis.
//!
//! The dump recurses over the tree. The tree depth is bounded by the
//! recursion budget of the parse; the dump uses less stack per level than
//! the parser charges (see [`crate::parser`]).

use std::fmt::Write;

use swb_js_text::Str16;

use crate::ast::{
    AssignOp, AssignTarget, BinaryOp, CatchClause, ClassElementKind, ClassId, ClassKey, Declarator,
    ExprId, ExprKind, ForHead, FunctionId, FunctionKind, List, LogicalOp, PatternId, PatternKind,
    Property, PropertyKey, PropertyKind, StmtId, StmtKind, TemplateId, UnaryOp, UpdateOp,
    VariableKind,
};
use crate::parser::Script;
use crate::scope::{CaptureSource, Resolution, Storage};

/// The tree of a script: one S-expression per top-level statement,
/// separated by spaces.
pub fn dump_ast(script: &Script) -> String {
    let mut dumper = Dumper {
        script,
        out: String::new(),
    };
    let body = script.ast.function(script.top).body;
    dumper.stmts(script.ast.stmts(body));
    dumper.out
}

struct Dumper<'a> {
    script: &'a Script,
    out: String,
}

impl Dumper<'_> {
    fn text(&mut self, text: &str) {
        self.out.push_str(text);
    }

    fn name(&mut self, name: crate::NameId) {
        let text = self.script.name_text(name);
        self.out.push_str(&text);
    }

    fn stmts(&mut self, stmts: &[StmtId]) {
        for (index, &stmt) in stmts.iter().enumerate() {
            if index > 0 {
                self.text(" ");
            }
            self.stmt(stmt);
        }
    }

    fn opt_expr(&mut self, expr: Option<ExprId>) {
        match expr {
            Some(expr) => self.expr(expr),
            None => self.text("_"),
        }
    }

    fn stmt(&mut self, id: StmtId) {
        let ast = &self.script.ast;
        match ast.stmt(id).kind {
            StmtKind::Empty => self.text("(empty)"),
            StmtKind::Debugger => self.text("(debugger)"),
            StmtKind::Expr(expr) => self.expr(expr),
            StmtKind::Block { body, .. } => {
                self.text("(block");
                for &stmt in ast.stmts(body) {
                    self.text(" ");
                    self.stmt(stmt);
                }
                self.text(")");
            }
            StmtKind::Variables { kind, declarators } => self.variables(kind, declarators),
            StmtKind::Function(function) => self.function(function),
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.text("(if ");
                self.expr(test);
                self.text(" ");
                self.stmt(consequent);
                if let Some(alternate) = alternate {
                    self.text(" ");
                    self.stmt(alternate);
                }
                self.text(")");
            }
            StmtKind::While { test, body } => {
                self.text("(while ");
                self.expr(test);
                self.text(" ");
                self.stmt(body);
                self.text(")");
            }
            StmtKind::DoWhile { body, test } => {
                self.text("(do ");
                self.stmt(body);
                self.text(" ");
                self.expr(test);
                self.text(")");
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
                ..
            } => self.for_stmt(init, test, update, body),
            StmtKind::ForIn {
                left, right, body, ..
            } => self.for_in_of("(for-in ", left, right, body),
            StmtKind::ForOf {
                left,
                right,
                body,
                is_await,
                ..
            } => self.for_in_of(for_of_head(is_await), left, right, body),
            StmtKind::Class(class) => self.class(class),
            StmtKind::With { object, body, .. } => self.with_stmt(object, body),
            StmtKind::Labeled { label, body } => {
                self.text("(label ");
                self.name(label);
                self.text(" ");
                self.stmt(body);
                self.text(")");
            }
            StmtKind::Break { label } => self.jump("(break", label),
            StmtKind::Continue { label } => self.jump("(continue", label),
            StmtKind::Return(argument) => {
                self.text("(return");
                if let Some(argument) = argument {
                    self.text(" ");
                    self.expr(argument);
                }
                self.text(")");
            }
            StmtKind::Throw(argument) => {
                self.text("(throw ");
                self.expr(argument);
                self.text(")");
            }
            StmtKind::Switch {
                discriminant,
                cases,
                ..
            } => self.switch_stmt(discriminant, cases),
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => self.try_stmt(block, handler, finalizer),
            kind @ (StmtKind::Import
            | StmtKind::ExportList
            | StmtKind::ExportDeclaration(_)
            | StmtKind::ExportDefault { .. }) => self.module_stmt(kind),
        }
    }

    /// The import and export statements.
    fn module_stmt(&mut self, kind: StmtKind) {
        match kind {
            StmtKind::Import => self.text("(import)"),
            StmtKind::ExportDeclaration(declaration) => {
                self.text("(export ");
                self.stmt(declaration);
                self.text(")");
            }
            StmtKind::ExportDefault { value, .. } => self.list("export-default", &[value]),
            _ => self.text("(export)"),
        }
    }

    fn jump(&mut self, head: &str, label: Option<crate::NameId>) {
        self.text(head);
        if let Some(label) = label {
            self.text(" ");
            self.name(label);
        }
        self.text(")");
    }

    fn variables(&mut self, kind: VariableKind, declarators: List<Declarator>) {
        let ast = &self.script.ast;
        self.text(match kind {
            VariableKind::Var => "(var",
            VariableKind::Let => "(let",
            VariableKind::Const => "(const",
        });
        for declarator in ast.declarators(declarators) {
            self.text(" ");
            match declarator.init {
                Some(init) => {
                    self.text("(");
                    self.pattern(declarator.target);
                    self.text(" ");
                    self.expr(init);
                    self.text(")");
                }
                None => self.pattern(declarator.target),
            }
        }
        self.text(")");
    }

    fn for_stmt(
        &mut self,
        init: Option<StmtId>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
    ) {
        self.text("(for ");
        match init {
            Some(init) => self.stmt(init),
            None => self.text("_"),
        }
        self.text(" ");
        self.opt_expr(test);
        self.text(" ");
        self.opt_expr(update);
        self.text(" ");
        self.stmt(body);
        self.text(")");
    }

    fn for_in_of(&mut self, head: &str, left: ForHead, right: ExprId, body: StmtId) {
        self.text(head);
        match left {
            ForHead::Declaration(stmt) => self.stmt(stmt),
            ForHead::Target(target) => self.assign_target(target),
        }
        self.text(" ");
        self.expr(right);
        self.text(" ");
        self.stmt(body);
        self.text(")");
    }

    fn with_stmt(&mut self, object: ExprId, body: StmtId) {
        self.text("(with ");
        self.expr(object);
        self.text(" ");
        self.stmt(body);
        self.text(")");
    }

    fn assign_target(&mut self, target: AssignTarget) {
        match target {
            AssignTarget::Simple(expr) => self.expr(expr),
            AssignTarget::Pattern(pattern) => self.pattern(pattern),
        }
    }

    fn switch_stmt(&mut self, discriminant: ExprId, cases: List<crate::ast::SwitchCase>) {
        let ast = &self.script.ast;
        self.text("(switch ");
        self.expr(discriminant);
        for case in ast.cases(cases) {
            self.text(" (");
            match case.test {
                Some(test) => {
                    self.text("case ");
                    self.expr(test);
                }
                None => self.text("default"),
            }
            for &stmt in ast.stmts(case.body) {
                self.text(" ");
                self.stmt(stmt);
            }
            self.text(")");
        }
        self.text(")");
    }

    fn try_stmt(&mut self, block: StmtId, handler: Option<CatchClause>, finalizer: Option<StmtId>) {
        self.text("(try ");
        self.stmt(block);
        if let Some(handler) = handler {
            self.text(" (catch ");
            match handler.param {
                Some(param) => self.pattern(param),
                None => self.text("_"),
            }
            self.text(" ");
            self.stmt(handler.body);
            self.text(")");
        }
        if let Some(finalizer) = finalizer {
            self.text(" (finally ");
            self.stmt(finalizer);
            self.text(")");
        }
        self.text(")");
    }

    /// A pattern: `[a _ ...b]` (array; `_` an elision), `{(key a) ...b}`
    /// (object), `(= target value)` (a default).
    fn pattern(&mut self, id: PatternId) {
        let ast = &self.script.ast;
        match ast.pattern(id).kind {
            PatternKind::Identifier(ident) => self.name(ident.name),
            PatternKind::Expr(expr) => self.expr(expr),
            PatternKind::Hole => self.text("_"),
            PatternKind::Rest(target) => {
                self.text("...");
                self.pattern(target);
            }
            PatternKind::Default { target, value } => {
                self.text("(= ");
                self.pattern(target);
                self.text(" ");
                self.expr(value);
                self.text(")");
            }
            PatternKind::Array(list) => {
                self.text("[");
                for (index, &element) in ast.patterns(list).iter().enumerate() {
                    if index > 0 {
                        self.text(" ");
                    }
                    self.pattern(element);
                }
                self.text("]");
            }
            PatternKind::Object { properties, rest } => {
                self.text("{");
                for (index, property) in ast.pattern_properties(properties).iter().enumerate() {
                    if index > 0 {
                        self.text(" ");
                    }
                    self.text("(");
                    self.key(property.key);
                    self.text(" ");
                    self.pattern(property.value);
                    self.text(")");
                }
                if let Some(rest) = rest {
                    if !properties.is_empty() {
                        self.text(" ");
                    }
                    self.text("...");
                    self.pattern(rest);
                }
                self.text("}");
            }
        }
    }

    fn key(&mut self, key: PropertyKey) {
        match key {
            PropertyKey::Name(name) => self.name(name),
            PropertyKey::Number(value) => {
                let _ = write!(self.out, "{value}");
            }
            PropertyKey::BigInt(digits) => self.bigint(digits),
            PropertyKey::Computed(key) => {
                self.text("[");
                self.expr(key);
                self.text("]");
            }
        }
    }

    /// A class: `(class NAME (extends X) (constructor F) (method KEY F)
    /// (static get KEY F) (field KEY VALUE) (static-block STMT...))`.
    fn class(&mut self, id: ClassId) {
        let ast = &self.script.ast;
        let class = ast.class(id);
        self.text("(class");
        if let Some(name) = class.name {
            self.text(" ");
            self.name(name);
        }
        if let Some(heritage) = class.heritage {
            self.text(" (extends ");
            self.expr(heritage);
            self.text(")");
        }
        if let Some(constructor) = class.constructor {
            self.text(" (constructor ");
            self.function(constructor);
            self.text(")");
        }
        for element in ast.class_elements(class.elements) {
            self.text(" (");
            if element.is_static {
                self.text("static ");
            }
            let head = match element.kind {
                ClassElementKind::Method => "method ",
                ClassElementKind::Getter => "get ",
                ClassElementKind::Setter => "set ",
                ClassElementKind::Field => "field ",
                ClassElementKind::StaticBlock { body, .. } => {
                    self.text("block");
                    for &stmt in ast.stmts(body) {
                        self.text(" ");
                        self.stmt(stmt);
                    }
                    self.text(")");
                    continue;
                }
            };
            self.text(head);
            match element.key {
                ClassKey::Property(key) => self.key(key),
                ClassKey::Private(ident) => self.name(ident.name),
                ClassKey::StaticBlock => {}
            }
            if let Some(value) = element.value {
                self.text(" ");
                self.expr(value);
            }
            self.text(")");
        }
        self.text(")");
    }

    fn function(&mut self, id: FunctionId) {
        let ast = &self.script.ast;
        let function = ast.function(id);
        let head = match (function.kind, function.is_generator) {
            (FunctionKind::Arrow, _) => "(=>",
            (FunctionKind::Method, false) => "(method",
            (FunctionKind::Method, true) => "(method*",
            (FunctionKind::Getter, _) => "(getter",
            (FunctionKind::Setter, _) => "(setter",
            (FunctionKind::ClassConstructor | FunctionKind::DerivedConstructor, _) => "(ctor",
            (_, false) => "(function",
            (_, true) => "(function*",
        };
        if function.is_async {
            self.text("(async ");
            self.text(&head[1..]);
        } else {
            self.text(head);
        }
        if let Some(name) = function.name {
            self.text(" ");
            self.name(name.name);
        }
        self.text(" (");
        for (index, &param) in ast.patterns(function.params).iter().enumerate() {
            if index > 0 {
                self.text(" ");
            }
            self.pattern(param);
        }
        self.text(")");
        let body = ast.stmts(function.body);
        if function.expression_body
            && let [stmt] = body
            && let StmtKind::Return(Some(value)) = ast.stmt(*stmt).kind
        {
            self.text(" ");
            self.expr(value);
        } else {
            for &stmt in body {
                self.text(" ");
                self.stmt(stmt);
            }
        }
        self.text(")");
    }

    fn list(&mut self, head: &str, items: &[ExprId]) {
        self.text("(");
        self.text(head);
        for &item in items {
            self.text(" ");
            self.expr(item);
        }
        self.text(")");
    }

    fn expr(&mut self, id: ExprId) {
        let ast = &self.script.ast;
        match ast.expr(id).kind {
            ExprKind::Identifier(ident) => self.name(ident.name),
            ExprKind::This(_) => self.text("this"),
            ExprKind::Null => self.text("null"),
            ExprKind::Boolean(value) => self.text(if value { "true" } else { "false" }),
            ExprKind::Number(value) => {
                let _ = write!(self.out, "{value}");
            }
            ExprKind::String(string) => self.string(ast.string(string).as_str16()),
            ExprKind::Template(template) => self.template(template),
            ExprKind::RegExp { pattern, flags } => {
                self.text("/");
                let text = ast.string(pattern).to_string_lossy();
                self.text(&text);
                self.text("/");
                self.text(&flags.letters().collect::<String>());
            }
            ExprKind::Array(elements) => self.list("array", ast.exprs(elements)),
            ExprKind::Hole => self.text("hole"),
            ExprKind::Object(properties) => self.object(properties),
            ExprKind::Function(function) => self.function(function),
            ExprKind::Paren(inner) => self.list("paren", &[inner]),
            ExprKind::Unary { op, argument } => self.list(unary_text(op), &[argument]),
            ExprKind::Update { op, prefix, target } => {
                let head = match (op, prefix) {
                    (UpdateOp::Increment, true) => "prefix++",
                    (UpdateOp::Increment, false) => "postfix++",
                    (UpdateOp::Decrement, true) => "prefix--",
                    (UpdateOp::Decrement, false) => "postfix--",
                };
                self.list(head, &[target]);
            }
            ExprKind::Binary { op, left, right } => self.list(binary_text(op), &[left, right]),
            ExprKind::Logical { op, left, right } => self.list(logical_text(op), &[left, right]),
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => self.list("?", &[test, consequent, alternate]),
            ExprKind::Assign { op, target, value } => self.assign(op, target, value),
            ExprKind::Sequence(list) => self.list(",", ast.exprs(list)),
            ExprKind::Member {
                object,
                property,
                optional,
            } => {
                self.text(if optional { "(?. " } else { "(. " });
                self.expr(object);
                self.text(" ");
                self.name(property);
                self.text(")");
            }
            ExprKind::Index {
                object,
                index,
                optional,
            } => self.list(if optional { "?.[]" } else { "[]" }, &[object, index]),
            ExprKind::Call {
                callee,
                arguments,
                optional,
            } => {
                let mut items = vec![callee];
                items.extend_from_slice(ast.exprs(arguments));
                self.list(if optional { "?.call" } else { "call" }, &items);
            }
            ExprKind::New { callee, arguments } => {
                let mut items = vec![callee];
                items.extend_from_slice(ast.exprs(arguments));
                self.list("new", &items);
            }
            ExprKind::Yield { argument, delegate } => match argument {
                Some(argument) => self.list(if delegate { "yield*" } else { "yield" }, &[argument]),
                None => self.text("(yield)"),
            },
            ExprKind::Spread(argument) => self.list("...", &[argument]),
            ExprKind::OptionalChain(chain) => self.list("chain", &[chain]),
            ExprKind::TaggedTemplate { tag, template } => self.tagged_template(tag, template),
            ExprKind::NewTarget(_) => self.text("new.target"),
            ExprKind::BigInt(digits) => self.bigint(digits),
            ExprKind::Class(_)
            | ExprKind::Await(_)
            | ExprKind::SuperMember { .. }
            | ExprKind::SuperIndex { .. }
            | ExprKind::SuperCall(_)
            | ExprKind::PrivateMember { .. }
            | ExprKind::PrivateIn { .. } => self.class_or_async_expr(ast.expr(id).kind),
            ExprKind::ImportCall { specifier, options } => {
                let mut items = vec![specifier];
                items.extend(options);
                self.list("import", &items);
            }
            ExprKind::ImportMeta => self.text("import.meta"),
        }
    }

    /// The expressions of classes, `super`, private names and `await`.
    fn class_or_async_expr(&mut self, kind: ExprKind) {
        let ast = &self.script.ast;
        match kind {
            ExprKind::Class(class) => self.class(class),
            ExprKind::Await(argument) => self.list("await", &[argument]),
            ExprKind::SuperMember { property, .. } => {
                self.text("(super. ");
                self.name(property);
                self.text(")");
            }
            ExprKind::SuperIndex { index, .. } => self.list("super[]", &[index]),
            ExprKind::SuperCall(call) => {
                let arguments = ast.super_call(call).arguments;
                self.list("super-call", ast.exprs(arguments));
            }
            ExprKind::PrivateMember {
                object,
                name,
                optional,
            } => {
                self.text(if optional { "(?. " } else { "(. " });
                self.expr(object);
                self.text(" ");
                self.name(name.name);
                self.text(")");
            }
            ExprKind::PrivateIn { name, object } => {
                self.text("(in ");
                self.name(name.name);
                self.text(" ");
                self.expr(object);
                self.text(")");
            }
            _ => {}
        }
    }

    fn assign(&mut self, op: AssignOp, target: AssignTarget, value: ExprId) {
        let head = match op {
            AssignOp::Assign => "=".to_owned(),
            AssignOp::Compound(op) => format!("{}=", binary_text(op)),
            AssignOp::Logical(op) => format!("{}=", logical_text(op)),
        };
        self.text("(");
        self.text(&head);
        self.text(" ");
        self.assign_target(target);
        self.text(" ");
        self.expr(value);
        self.text(")");
    }

    fn tagged_template(&mut self, tag: ExprId, template: TemplateId) {
        self.text("(tag ");
        self.expr(tag);
        self.text(" ");
        self.template(template);
        self.text(")");
    }

    fn bigint(&mut self, digits: crate::StringId) {
        let text = self.script.ast.string(digits).to_string_lossy();
        self.text(&text);
        self.text("n");
    }

    fn object(&mut self, properties: List<Property>) {
        let ast = &self.script.ast;
        self.text("(object");
        for property in ast.properties(properties) {
            self.text(" (");
            self.text(match property.kind {
                PropertyKind::Init => "init ",
                PropertyKind::Proto => "proto ",
                PropertyKind::Shorthand => "shorthand ",
                PropertyKind::Method => "method ",
                PropertyKind::Getter => "get ",
                PropertyKind::Setter => "set ",
                PropertyKind::Spread => "...",
            });
            if property.kind != PropertyKind::Spread {
                self.key(property.key);
            }
            if property.kind == PropertyKind::Spread {
                self.expr(property.value);
            } else if property.kind != PropertyKind::Shorthand
                || matches!(ast.expr(property.value).kind, ExprKind::Assign { .. })
            {
                self.text(" ");
                self.expr(property.value);
            }
            self.text(")");
        }
        self.text(")");
    }

    fn template(&mut self, id: TemplateId) {
        let ast = &self.script.ast;
        let template = ast.template(id);
        let quasis = ast.template_elements(template.quasis);
        let expressions = ast.exprs(template.expressions);
        self.text("(`");
        for (index, quasi) in quasis.iter().enumerate() {
            self.text(" ");
            match quasi.cooked {
                Some(cooked) => self.string(ast.string(cooked).as_str16()),
                None => self.text("_"),
            }
            if let Some(&expression) = expressions.get(index) {
                self.text(" ");
                self.expr(expression);
            }
        }
        self.text(")");
    }

    fn string(&mut self, value: Str16<'_>) {
        self.text("\"");
        for c in value.to_string_lossy().chars() {
            match c {
                '"' => self.text("\\\""),
                '\\' => self.text("\\\\"),
                '\n' => self.text("\\n"),
                c if c.is_control() => {
                    let _ = write!(self.out, "\\u{{{:x}}}", u32::from(c));
                }
                c => self.out.push(c),
            }
        }
        self.text("\"");
    }
}

fn for_of_head(is_await: bool) -> &'static str {
    if is_await {
        "(for-await-of "
    } else {
        "(for-of "
    }
}

fn unary_text(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Delete => "delete",
        UnaryOp::Void => "void",
        UnaryOp::Typeof => "typeof",
        UnaryOp::Plus => "+",
        UnaryOp::Minus => "-",
        UnaryOp::BitNot => "~",
        UnaryOp::Not => "!",
    }
}

fn binary_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Exponent => "**",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Remainder => "%",
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::ShiftRightUnsigned => ">>>",
        BinaryOp::Less => "<",
        BinaryOp::Greater => ">",
        BinaryOp::LessEqual => "<=",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Instanceof => "instanceof",
        BinaryOp::In => "in",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::StrictEqual => "===",
        BinaryOp::StrictNotEqual => "!==",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitXor => "^",
        BinaryOp::BitOr => "|",
    }
}

fn logical_text(op: LogicalOp) -> &'static str {
    match op {
        LogicalOp::And => "&&",
        LogicalOp::Or => "||",
        LogicalOp::Coalesce => "??",
    }
}

/// The results of the scope analysis, one line per function, scope and
/// binding:
///
/// ```text
/// function 1 f parent 0 params [a] registers 2 captures [x<-r0] this arguments eval
///   scope 2 function: a r0 parameter, b cell r1 let
/// ```
pub fn dump_scopes(script: &Script) -> String {
    let mut out = String::new();
    let tree = &script.scopes;
    for id in script.ast.function_ids() {
        let function = script.ast.function(id);
        let info = tree.function(id);
        let _ = write!(out, "function {}", id.index());
        if let Some(name) = function.name {
            let _ = write!(out, " {}", script.name_text(name.name));
        }
        if let Some(parent) = function.parent {
            let _ = write!(out, " parent {}", parent.index());
        }
        let params: Vec<String> = info
            .params
            .iter()
            .map(|&b| script.name_text(tree.binding(b).name))
            .collect();
        let _ = write!(
            out,
            " params [{}] registers {}",
            params.join(" "),
            info.register_count
        );
        if !info.captures.is_empty() {
            let captures: Vec<String> = info
                .captures
                .iter()
                .map(|capture| {
                    let name = script.name_text(tree.binding(capture.binding).name);
                    match capture.source {
                        CaptureSource::ParentRegister(r) => format!("{name}<-r{r}"),
                        CaptureSource::ParentCapture(i) => format!("{name}<-c{i}"),
                        CaptureSource::Import(i) => format!("{name}<-import{i}"),
                    }
                })
                .collect();
            let _ = write!(out, " captures [{}]", captures.join(" "));
        }
        if info.this_binding.is_some() {
            out.push_str(" this");
        }
        if info.arguments_binding.is_some() {
            out.push_str(" arguments");
            if info.mapped_arguments {
                out.push_str(" mapped");
            }
        }
        if info.needs_scope_description {
            out.push_str(" eval");
        }
        out.push('\n');
        for index in 0..tree.scope_count() {
            let scope_id = crate::ast::ScopeId::from_index(index);
            let scope = tree.scope(scope_id);
            if scope.function != id || tree.bindings_of(scope_id).is_empty() {
                continue;
            }
            let _ = write!(out, "  scope {} {:?}:", index, scope.kind);
            if scope.per_iteration {
                out.push_str(" per-iteration");
            }
            for (n, &binding) in tree.bindings_of(scope_id).iter().enumerate() {
                let binding = tree.binding(binding);
                let storage = match binding.storage {
                    Storage::Unassigned => "?".to_owned(),
                    Storage::Register(r) => format!("r{r}"),
                    Storage::Cell(r) => format!("cell r{r}"),
                    Storage::Global => "global".to_owned(),
                    Storage::Caller => "caller".to_owned(),
                    Storage::Import => "import".to_owned(),
                };
                let _ = write!(
                    out,
                    "{} {} {} {:?}",
                    if n == 0 { "" } else { "," },
                    script.name_text(binding.name),
                    storage,
                    binding.kind
                );
            }
            out.push('\n');
        }
    }
    out
}

/// The resolution of every identifier occurrence, as `name@offset:how`,
/// where `how` is `rN`, `cellN`, `capN`, `global` or `dead`, with `!` for a TDZ
/// check and `=` for a declaration.
pub fn dump_references(script: &Script) -> String {
    let tree = &script.scopes;
    let mut parts = Vec::new();
    for index in 0..tree.reference_count() {
        let reference = tree.reference(crate::ast::RefId::from_index(index));
        let how = match reference.resolution {
            Resolution::Unresolved if reference.dead => "dead".to_owned(),
            Resolution::Unresolved => "?".to_owned(),
            Resolution::Register(r) => format!("r{r}"),
            Resolution::Cell(r) => format!("cell{r}"),
            Resolution::Capture(i) => format!("cap{i}"),
            Resolution::Global => "global".to_owned(),
            Resolution::Caller => "caller".to_owned(),
        };
        let dynamic = tree
            .dynamic_lookup(crate::ast::RefId::from_index(index))
            .map(|lookup| format!("~{}", lookup.count))
            .unwrap_or_default();
        parts.push(format!(
            "{}@{}:{}{}{}{}",
            script.name_text(reference.name),
            reference.offset,
            if reference.declaration { "=" } else { "" },
            how,
            if reference.tdz_check { "!" } else { "" },
            dynamic,
        ));
    }
    parts.join(" ")
}
