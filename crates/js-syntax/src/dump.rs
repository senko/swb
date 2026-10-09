//! Readable dumps of a parsed script, for tests and debugging: the tree
//! as S-expressions, and the results of the scope analysis.
//!
//! The dump recurses over the tree. The tree depth is bounded by the
//! recursion budget of the parse; the dump uses less stack per level than
//! the parser charges (see [`crate::parser`]).

use std::fmt::Write;

use swb_js_regexp::Flags;
use swb_js_text::Str16;

use crate::ast::{
    AssignOp, AssignTarget, BinaryOp, CatchClause, Declarator, ExprId, ExprKind, FunctionId,
    FunctionKind, List, LogicalOp, PatternId, PatternKind, Property, PropertyKey, PropertyKind,
    StmtId, StmtKind, TemplateId, UnaryOp, UpdateOp, VariableKind,
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
            } => {
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
            StmtKind::Labeled { label, body } => {
                self.text("(label ");
                self.name(label);
                self.text(" ");
                self.stmt(body);
                self.text(")");
            }
            StmtKind::Break { label } | StmtKind::Continue { label } => {
                let is_break = matches!(ast.stmt(id).kind, StmtKind::Break { .. });
                self.text(if is_break { "(break" } else { "(continue" });
                if let Some(label) = label {
                    self.text(" ");
                    self.name(label);
                }
                self.text(")");
            }
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
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => self.try_stmt(block, handler, finalizer),
        }
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

    fn pattern(&mut self, id: PatternId) {
        let PatternKind::Identifier(ident) = self.script.ast.pattern(id).kind;
        self.name(ident.name);
    }

    fn function(&mut self, id: FunctionId) {
        let ast = &self.script.ast;
        let function = ast.function(id);
        let head = match (function.kind, function.is_generator) {
            (FunctionKind::Arrow, _) => "(=>",
            (FunctionKind::Method, false) => "(method",
            (FunctionKind::Method, true) => "(method*",
            (_, false) => "(function",
            (_, true) => "(function*",
        };
        self.text(head);
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
                self.text(&flags_text(flags));
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
            ExprKind::Assign { op, target, value } => {
                let AssignTarget::Simple(target) = target;
                let head = match op {
                    AssignOp::Assign => "=".to_owned(),
                    AssignOp::Compound(op) => format!("{}=", binary_text(op)),
                    AssignOp::Logical(op) => format!("{}=", logical_text(op)),
                };
                self.list(&head, &[target, value]);
            }
            ExprKind::Sequence(list) => self.list(",", ast.exprs(list)),
            ExprKind::Member { object, property } => {
                self.text("(. ");
                self.expr(object);
                self.text(" ");
                self.name(property);
                self.text(")");
            }
            ExprKind::Index { object, index } => self.list("[]", &[object, index]),
            ExprKind::Call { callee, arguments } => {
                let mut items = vec![callee];
                items.extend_from_slice(ast.exprs(arguments));
                self.list("call", &items);
            }
            ExprKind::New { callee, arguments } => {
                let mut items = vec![callee];
                items.extend_from_slice(ast.exprs(arguments));
                self.list("new", &items);
            }
            ExprKind::Yield { argument } => match argument {
                Some(argument) => self.list("yield", &[argument]),
                None => self.text("(yield)"),
            },
        }
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
            });
            match property.key {
                PropertyKey::Name(name) => self.name(name),
                PropertyKey::Number(value) => {
                    let _ = write!(self.out, "{value}");
                }
                PropertyKey::Computed(key) => {
                    self.text("[");
                    self.expr(key);
                    self.text("]");
                }
            }
            if property.kind != PropertyKind::Shorthand {
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

fn flags_text(flags: Flags) -> String {
    [
        (Flags::HAS_INDICES, 'd'),
        (Flags::GLOBAL, 'g'),
        (Flags::IGNORE_CASE, 'i'),
        (Flags::MULTILINE, 'm'),
        (Flags::DOT_ALL, 's'),
        (Flags::UNICODE, 'u'),
        (Flags::UNICODE_SETS, 'v'),
        (Flags::STICKY, 'y'),
    ]
    .iter()
    .filter(|(flag, _)| flags.contains(*flag))
    .map(|&(_, c)| c)
    .collect()
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
/// where `how` is `rN`, `cellN`, `capN` or `global`, with `!` for a TDZ
/// check and `=` for a declaration.
pub fn dump_references(script: &Script) -> String {
    let tree = &script.scopes;
    let mut parts = Vec::new();
    for index in 0..tree.reference_count() {
        let reference = tree.reference(crate::ast::RefId::from_index(index));
        let how = match reference.resolution {
            Resolution::Unresolved => "?".to_owned(),
            Resolution::Register(r) => format!("r{r}"),
            Resolution::Cell(r) => format!("cell{r}"),
            Resolution::Capture(i) => format!("cap{i}"),
            Resolution::Global => "global".to_owned(),
        };
        parts.push(format!(
            "{}@{}:{}{}{}",
            script.name_text(reference.name),
            reference.offset,
            if reference.declaration { "=" } else { "" },
            how,
            if reference.tdz_check { "!" } else { "" },
        ));
    }
    parts.join(" ")
}
