//! A recursive-descent parser for the Web IDL subset of ADR 0028 section 5
//! (<https://webidl.spec.whatwg.org/#idl-grammar>): interfaces, partial
//! interfaces, mixins and `includes`, callback interfaces, dictionaries,
//! enumerations, callback functions and typedefs, with constants,
//! attributes, operations, constructors, special operations, stringifiers
//! and `iterable` declarations. Namespaces, `maplike`, `setlike`,
//! `async_iterable` and inherited attributes are not in the subset and
//! give an error. Every error names the file and the line.

use crate::Error;
use crate::ast::{
    Argument, Attribute, CallbackFunction, Const, ConstValue, Definition, Dictionary,
    DictionaryMember, Enum, ExtAttr, ExtAttrs, ExtValue, FloatType, Includes, IntegerType,
    Interface, Iterable, Member, Operation, Place, Special, Type, TypeKind, Typedef,
};
use crate::lexer::{Token, TokenKind, tokenize};

/// Parses the definitions of one file.
pub fn parse(file: &str, text: &str) -> std::result::Result<Vec<Definition>, Error> {
    let tokens = tokenize(file, text)?;
    let mut parser = Parser {
        file,
        tokens,
        pos: 0,
    };
    let mut definitions = Vec::new();
    while !parser.at_end() {
        definitions.push(parser.definition()?);
    }
    Ok(definitions)
}

struct Parser<'a> {
    file: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

type Result<T> = std::result::Result<T, Error>;

impl Parser<'_> {
    // --- Tokens ---

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn peek_at(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + offset).map(|t| &t.kind)
    }

    /// The line of the current token (of the last one at the end).
    fn line(&self) -> u32 {
        self.tokens
            .get(self.pos)
            .or_else(|| self.tokens.last())
            .map_or(1, |t| t.line)
    }

    fn place(&self) -> Place {
        Place {
            file: self.file.to_owned(),
            line: self.line(),
        }
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(Error {
            file: self.file.to_owned(),
            line: self.line(),
            message: message.into(),
        })
    }

    fn describe(&self) -> String {
        match self.peek() {
            None => "the end of the file".to_owned(),
            Some(TokenKind::Ident(name)) => format!("'{name}'"),
            Some(TokenKind::Integer(value)) => format!("'{value}'"),
            Some(TokenKind::Decimal(text)) => format!("'{text}'"),
            Some(TokenKind::String(text)) => format!("\"{text}\""),
            Some(TokenKind::Ellipsis) => "'...'".to_owned(),
            Some(TokenKind::Other(c)) => format!("'{c}'"),
        }
    }

    fn is_char(&self, c: char) -> bool {
        self.peek() == Some(&TokenKind::Other(c))
    }

    fn eat_char(&mut self, c: char) -> bool {
        if self.is_char(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_char(&mut self, c: char) -> Result<()> {
        if self.eat_char(c) {
            Ok(())
        } else {
            self.error(format!("expected '{c}', found {}", self.describe()))
        }
    }

    fn is_keyword(&self, word: &str) -> bool {
        matches!(self.peek(), Some(TokenKind::Ident(name)) if name == word)
    }

    fn eat_keyword(&mut self, word: &str) -> bool {
        if self.is_keyword(word) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_keyword(&mut self, word: &str) -> Result<()> {
        if self.eat_keyword(word) {
            Ok(())
        } else {
            self.error(format!("expected '{word}', found {}", self.describe()))
        }
    }

    fn identifier(&mut self) -> Result<String> {
        match self.peek() {
            Some(TokenKind::Ident(name)) => {
                let name = name.clone();
                self.pos += 1;
                Ok(name)
            }
            _ => self.error(format!("expected a name, found {}", self.describe())),
        }
    }

    // --- Definitions ---

    fn definition(&mut self) -> Result<Definition> {
        let ext_attrs = self.ext_attrs()?;
        let place = self.place();
        if self.eat_keyword("callback") {
            if self.eat_keyword("interface") {
                let mut interface = self.interface_body(ext_attrs, place, false)?;
                interface.callback = true;
                return Ok(Definition::Interface(interface));
            }
            return self.callback_function(ext_attrs, place);
        }
        if self.eat_keyword("partial") {
            if self.eat_keyword("interface") {
                let mixin = self.eat_keyword("mixin");
                let mut interface = self.interface_body(ext_attrs, place, mixin)?;
                interface.partial = true;
                return Ok(Definition::Interface(interface));
            }
            if self.eat_keyword("dictionary") {
                let mut dictionary = self.dictionary(ext_attrs, place)?;
                dictionary.partial = true;
                return Ok(Definition::Dictionary(dictionary));
            }
            return self.error(format!(
                "expected 'interface' or 'dictionary' after 'partial', found {}",
                self.describe()
            ));
        }
        if self.eat_keyword("interface") {
            let mixin = self.eat_keyword("mixin");
            return Ok(Definition::Interface(
                self.interface_body(ext_attrs, place, mixin)?,
            ));
        }
        if self.eat_keyword("dictionary") {
            return Ok(Definition::Dictionary(self.dictionary(ext_attrs, place)?));
        }
        if self.eat_keyword("enum") {
            return self.enumeration(place);
        }
        if self.eat_keyword("typedef") {
            let ty = self.type_with_ext_attrs()?;
            let name = self.identifier()?;
            self.expect_char(';')?;
            return Ok(Definition::Typedef(Typedef { name, ty, place }));
        }
        if self.is_keyword("namespace") {
            return self.error("namespaces are not in the WebIDL subset of swb");
        }
        if let Some(TokenKind::Ident(_)) = self.peek()
            && matches!(self.peek_at(1), Some(TokenKind::Ident(word)) if word == "includes")
        {
            let target = self.identifier()?;
            self.expect_keyword("includes")?;
            let mixin = self.identifier()?;
            self.expect_char(';')?;
            return Ok(Definition::Includes(Includes {
                target,
                mixin,
                place,
            }));
        }
        self.error(format!("expected a definition, found {}", self.describe()))
    }

    fn interface_body(
        &mut self,
        ext_attrs: ExtAttrs,
        place: Place,
        mixin: bool,
    ) -> Result<Interface> {
        let name = self.identifier()?;
        let inherits = if self.eat_char(':') {
            Some(self.identifier()?)
        } else {
            None
        };
        self.expect_char('{')?;
        let mut members = Vec::new();
        while !self.eat_char('}') {
            if self.at_end() {
                return self.error(format!("the interface '{name}' is not closed"));
            }
            members.push(self.member()?);
        }
        self.expect_char(';')?;
        Ok(Interface {
            name,
            inherits,
            partial: false,
            mixin,
            callback: false,
            ext_attrs,
            members,
            place,
        })
    }

    fn member(&mut self) -> Result<Member> {
        let ext_attrs = self.ext_attrs()?;
        let place = self.place();
        if self.eat_keyword("const") {
            return self.constant(ext_attrs, place);
        }
        if self.eat_keyword("constructor") {
            let args = self.argument_list()?;
            self.expect_char(';')?;
            return Ok(Member::Constructor(Operation {
                name: None,
                ret: plain(TypeKind::Undefined),
                args,
                special: None,
                is_static: false,
                ext_attrs,
                place,
            }));
        }
        if self.eat_keyword("stringifier") {
            if self.eat_char(';') {
                return Ok(Member::Stringifier(place));
            }
            let mut attribute = self.attribute_rest(ext_attrs, place)?;
            attribute.stringifier = true;
            return Ok(Member::Attribute(attribute));
        }
        if self.is_keyword("iterable") {
            self.pos += 1;
            return self.iterable(place);
        }
        for word in ["maplike", "setlike", "async_iterable", "inherit"] {
            if self.is_keyword(word)
                || (self.is_keyword("readonly")
                    && matches!(self.peek_at(1), Some(TokenKind::Ident(w)) if w == word))
            {
                return self.error(format!("'{word}' is not in the WebIDL subset of swb"));
            }
        }
        let is_static = self.eat_keyword("static");
        if self.is_keyword("readonly") || self.is_keyword("attribute") {
            let mut attribute = self.attribute_rest(ext_attrs, place)?;
            attribute.is_static = is_static;
            return Ok(Member::Attribute(attribute));
        }
        let special = if self.eat_keyword("getter") {
            Some(Special::Getter)
        } else if self.eat_keyword("setter") {
            Some(Special::Setter)
        } else if self.eat_keyword("deleter") {
            Some(Special::Deleter)
        } else {
            None
        };
        let ret = self.type_with_ext_attrs()?;
        let name = match self.peek() {
            Some(TokenKind::Ident(_)) => Some(self.identifier()?),
            _ if special.is_some() => None,
            _ => {
                return self.error(format!(
                    "expected an operation name, found {}",
                    self.describe()
                ));
            }
        };
        let args = self.argument_list()?;
        self.expect_char(';')?;
        Ok(Member::Operation(Operation {
            name,
            ret,
            args,
            special,
            is_static,
            ext_attrs,
            place,
        }))
    }

    fn attribute_rest(&mut self, ext_attrs: ExtAttrs, place: Place) -> Result<Attribute> {
        let readonly = self.eat_keyword("readonly");
        self.expect_keyword("attribute")?;
        let ty = self.type_with_ext_attrs()?;
        let name = self.identifier()?;
        self.expect_char(';')?;
        Ok(Attribute {
            name,
            ty,
            readonly,
            is_static: false,
            stringifier: false,
            ext_attrs,
            place,
        })
    }

    fn iterable(&mut self, place: Place) -> Result<Member> {
        self.expect_char('<')?;
        let first = self.type_with_ext_attrs()?;
        let (key, value) = if self.eat_char(',') {
            (Some(first), self.type_with_ext_attrs()?)
        } else {
            (None, first)
        };
        self.expect_char('>')?;
        self.expect_char(';')?;
        Ok(Member::Iterable(Iterable { key, value, place }))
    }

    fn constant(&mut self, ext_attrs: ExtAttrs, place: Place) -> Result<Member> {
        let ty = self.type_()?;
        let name = self.identifier()?;
        self.expect_char('=')?;
        let value = self.literal()?;
        if !matches!(
            value,
            ConstValue::Bool(_) | ConstValue::Integer(_) | ConstValue::Float(_)
        ) {
            return self.error(format!("the constant '{name}' needs a boolean or a number"));
        }
        self.expect_char(';')?;
        Ok(Member::Const(Const {
            name,
            ty,
            value,
            ext_attrs,
            place,
        }))
    }

    fn dictionary(&mut self, ext_attrs: ExtAttrs, place: Place) -> Result<Dictionary> {
        let name = self.identifier()?;
        let inherits = if self.eat_char(':') {
            Some(self.identifier()?)
        } else {
            None
        };
        self.expect_char('{')?;
        let mut members = Vec::new();
        while !self.eat_char('}') {
            if self.at_end() {
                return self.error(format!("the dictionary '{name}' is not closed"));
            }
            let mut member_attrs = self.ext_attrs()?;
            let member_place = self.place();
            let required = self.eat_keyword("required");
            let mut ty = self.type_with_ext_attrs()?;
            move_type_attrs(&mut member_attrs, &mut ty);
            let member_name = self.identifier()?;
            let default = if self.eat_char('=') {
                Some(self.literal()?)
            } else {
                None
            };
            if required && default.is_some() {
                return self.error(format!(
                    "the required member '{member_name}' has a default value"
                ));
            }
            self.expect_char(';')?;
            members.push(DictionaryMember {
                name: member_name,
                ty,
                required,
                default,
                ext_attrs: member_attrs,
                place: member_place,
            });
        }
        self.expect_char(';')?;
        Ok(Dictionary {
            name,
            inherits,
            partial: false,
            members,
            ext_attrs,
            place,
        })
    }

    fn enumeration(&mut self, place: Place) -> Result<Definition> {
        let name = self.identifier()?;
        self.expect_char('{')?;
        let mut values = Vec::new();
        loop {
            match self.peek() {
                Some(TokenKind::String(text)) => {
                    values.push(text.clone());
                    self.pos += 1;
                }
                _ => {
                    return self.error(format!(
                        "expected a string in the enumeration '{name}', found {}",
                        self.describe()
                    ));
                }
            }
            if !self.eat_char(',') || self.is_char('}') {
                break;
            }
        }
        self.expect_char('}')?;
        self.expect_char(';')?;
        Ok(Definition::Enum(Enum {
            name,
            values,
            place,
        }))
    }

    fn callback_function(&mut self, ext_attrs: ExtAttrs, place: Place) -> Result<Definition> {
        let name = self.identifier()?;
        self.expect_char('=')?;
        let ret = self.type_with_ext_attrs()?;
        let args = self.argument_list()?;
        self.expect_char(';')?;
        Ok(Definition::Callback(CallbackFunction {
            name,
            ret,
            args,
            ext_attrs,
            place,
        }))
    }

    // --- Arguments ---

    fn argument_list(&mut self) -> Result<Vec<Argument>> {
        self.expect_char('(')?;
        let mut args = Vec::new();
        if self.eat_char(')') {
            return Ok(args);
        }
        loop {
            args.push(self.argument()?);
            if !self.eat_char(',') {
                break;
            }
        }
        self.expect_char(')')?;
        Ok(args)
    }

    fn argument(&mut self) -> Result<Argument> {
        let mut ext_attrs = self.ext_attrs()?;
        let optional = self.eat_keyword("optional");
        let mut ty = self.type_with_ext_attrs()?;
        move_type_attrs(&mut ext_attrs, &mut ty);
        let variadic = !optional && self.peek() == Some(&TokenKind::Ellipsis);
        if variadic {
            self.pos += 1;
        }
        let name = self.identifier()?;
        let default = if optional && self.eat_char('=') {
            Some(self.literal()?)
        } else {
            None
        };
        Ok(Argument {
            name,
            ty,
            optional,
            variadic,
            default,
            ext_attrs,
        })
    }

    fn literal(&mut self) -> Result<ConstValue> {
        let value = match self.peek() {
            Some(TokenKind::Integer(value)) => ConstValue::Integer(*value),
            Some(TokenKind::Decimal(text)) => ConstValue::Float(text.clone()),
            Some(TokenKind::String(text)) => ConstValue::String(text.clone()),
            Some(TokenKind::Ident(word)) => match word.as_str() {
                "true" => ConstValue::Bool(true),
                "false" => ConstValue::Bool(false),
                "null" => ConstValue::Null,
                "undefined" => ConstValue::Undefined,
                "Infinity" | "-Infinity" | "NaN" => ConstValue::Float(word.clone()),
                _ => return self.error(format!("expected a value, found {}", self.describe())),
            },
            Some(TokenKind::Other('{')) => {
                self.pos += 1;
                self.expect_char('}')?;
                return Ok(ConstValue::EmptyDictionary);
            }
            Some(TokenKind::Other('[')) => {
                self.pos += 1;
                self.expect_char(']')?;
                return Ok(ConstValue::EmptySequence);
            }
            Some(TokenKind::Other('-')) if matches!(self.peek_at(1), Some(TokenKind::Ident(w)) if w == "Infinity") =>
            {
                self.pos += 2;
                return Ok(ConstValue::Float("-Infinity".to_owned()));
            }
            _ => return self.error(format!("expected a value, found {}", self.describe())),
        };
        self.pos += 1;
        Ok(value)
    }

    // --- Types ---

    /// A type with extended attributes before it (`[Clamp] long`).
    fn type_with_ext_attrs(&mut self) -> Result<Type> {
        let ext_attrs = self.ext_attrs()?;
        let mut ty = self.type_()?;
        ty.ext_attrs.0.extend(ext_attrs.0);
        Ok(ty)
    }

    fn type_(&mut self) -> Result<Type> {
        let kind = if self.eat_char('(') {
            let mut members = vec![self.type_with_ext_attrs()?];
            while self.eat_keyword("or") {
                members.push(self.type_with_ext_attrs()?);
            }
            self.expect_char(')')?;
            if members.len() < 2 {
                return self.error("a union type needs at least two member types");
            }
            TypeKind::Union(members)
        } else {
            self.single_type()?
        };
        let nullable = self.eat_char('?');
        Ok(Type {
            kind,
            nullable,
            ext_attrs: ExtAttrs::default(),
        })
    }

    fn single_type(&mut self) -> Result<TypeKind> {
        let word = self.identifier()?;
        Ok(match word.as_str() {
            "any" => TypeKind::Any,
            "undefined" => TypeKind::Undefined,
            "boolean" => TypeKind::Boolean,
            "byte" => TypeKind::Integer(IntegerType::Byte),
            "octet" => TypeKind::Integer(IntegerType::Octet),
            "short" => TypeKind::Integer(IntegerType::Short),
            "long" => {
                if self.eat_keyword("long") {
                    TypeKind::Integer(IntegerType::LongLong)
                } else {
                    TypeKind::Integer(IntegerType::Long)
                }
            }
            "unsigned" => {
                if self.eat_keyword("short") {
                    TypeKind::Integer(IntegerType::UnsignedShort)
                } else if self.eat_keyword("long") {
                    if self.eat_keyword("long") {
                        TypeKind::Integer(IntegerType::UnsignedLongLong)
                    } else {
                        TypeKind::Integer(IntegerType::UnsignedLong)
                    }
                } else {
                    return self.error("expected 'short' or 'long' after 'unsigned'");
                }
            }
            "float" => TypeKind::Float(FloatType::Float),
            "double" => TypeKind::Float(FloatType::Double),
            "unrestricted" => {
                if self.eat_keyword("float") {
                    TypeKind::Float(FloatType::UnrestrictedFloat)
                } else if self.eat_keyword("double") {
                    TypeKind::Float(FloatType::UnrestrictedDouble)
                } else {
                    return self.error("expected 'float' or 'double' after 'unrestricted'");
                }
            }
            "bigint" => TypeKind::BigInt,
            "DOMString" | "CSSOMString" => TypeKind::DomString,
            "ByteString" => TypeKind::ByteString,
            "USVString" => TypeKind::UsvString,
            "object" => TypeKind::Object,
            "symbol" => TypeKind::Symbol,
            "sequence" => TypeKind::Sequence(Box::new(self.type_argument()?)),
            "FrozenArray" => TypeKind::FrozenArray(Box::new(self.type_argument()?)),
            "Promise" => TypeKind::Promise(Box::new(self.type_argument()?)),
            "record" => {
                self.expect_char('<')?;
                let key = self.type_with_ext_attrs()?;
                self.expect_char(',')?;
                let value = self.type_with_ext_attrs()?;
                self.expect_char('>')?;
                TypeKind::Record(Box::new(key), Box::new(value))
            }
            "ObservableArray" | "async_sequence" => {
                return self.error(format!("'{word}' is not in the WebIDL subset of swb"));
            }
            _ => TypeKind::Named(word),
        })
    }

    fn type_argument(&mut self) -> Result<Type> {
        self.expect_char('<')?;
        let ty = self.type_with_ext_attrs()?;
        self.expect_char('>')?;
        Ok(ty)
    }

    // --- Extended attributes ---

    fn ext_attrs(&mut self) -> Result<ExtAttrs> {
        let mut attrs = Vec::new();
        if !self.eat_char('[') {
            return Ok(ExtAttrs(attrs));
        }
        loop {
            let place = self.place();
            let name = self.identifier()?;
            let value = if self.eat_char('=') {
                self.ext_value()?
            } else {
                ExtValue::None
            };
            let args = if self.is_char('(') {
                Some(self.argument_list()?)
            } else {
                None
            };
            attrs.push(ExtAttr {
                name,
                value,
                args,
                place,
            });
            if !self.eat_char(',') {
                break;
            }
        }
        self.expect_char(']')?;
        Ok(ExtAttrs(attrs))
    }

    fn ext_value(&mut self) -> Result<ExtValue> {
        if self.eat_char('*') {
            return Ok(ExtValue::Ident("*".to_owned()));
        }
        if self.eat_char('(') {
            let mut items = Vec::new();
            if !self.eat_char(')') {
                loop {
                    items.push(self.ext_value()?);
                    if !self.eat_char(',') {
                        break;
                    }
                }
                self.expect_char(')')?;
            }
            return Ok(ExtValue::List(items));
        }
        let value = match self.peek() {
            Some(TokenKind::Ident(name)) => ExtValue::Ident(name.clone()),
            Some(TokenKind::String(text)) => ExtValue::String(text.clone()),
            Some(TokenKind::Integer(value)) => ExtValue::Integer(*value),
            Some(TokenKind::Decimal(text)) => ExtValue::Decimal(text.clone()),
            _ => {
                return self.error(format!(
                    "expected an extended attribute value, found {}",
                    self.describe()
                ));
            }
        };
        self.pos += 1;
        Ok(value)
    }
}

/// Moves the extended attributes that apply to a type (`[Clamp]`,
/// `[EnforceRange]`, `[LegacyNullToEmptyString]`, `[AllowShared]`) from
/// an argument or dictionary member to its type.
fn move_type_attrs(attrs: &mut ExtAttrs, ty: &mut Type) {
    let (type_attrs, rest): (Vec<ExtAttr>, Vec<ExtAttr>) =
        std::mem::take(&mut attrs.0).into_iter().partition(|a| {
            matches!(
                a.name.as_str(),
                "Clamp" | "EnforceRange" | "LegacyNullToEmptyString" | "AllowShared"
            )
        });
    attrs.0 = rest;
    ty.ext_attrs.0.extend(type_attrs);
}

/// A type without `?` or extended attributes.
fn plain(kind: TypeKind) -> Type {
    Type {
        kind,
        nullable: false,
        ext_attrs: ExtAttrs::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Definition {
        let mut defs = parse("t.idl", text).unwrap();
        assert_eq!(defs.len(), 1);
        defs.remove(0)
    }

    fn interface(text: &str) -> Interface {
        match one(text) {
            Definition::Interface(interface) => interface,
            other => panic!("not an interface: {other:?}"),
        }
    }

    #[test]
    fn an_interface_with_each_kind_of_member() {
        let i = interface(
            "[Exposed=Window] interface Node : EventTarget {
               constructor(optional DOMString data = \"\");
               const unsigned short ELEMENT_NODE = 1;
               readonly attribute unsigned short nodeType;
               [CEReactions] attribute DOMString? textContent;
               [CEReactions, NewObject] Node cloneNode(optional boolean subtree = false);
               undefined append((Node or DOMString)... nodes);
               getter Node? item(unsigned long index);
               stringifier;
               static boolean check(long x);
               iterable<Node>;
             };",
        );
        assert_eq!(i.name, "Node");
        assert_eq!(i.inherits.as_deref(), Some("EventTarget"));
        assert!(i.ext_attrs.has("Exposed"));
        assert_eq!(i.members.len(), 10);
        let Member::Operation(clone) = &i.members[4] else {
            panic!()
        };
        assert_eq!(clone.name.as_deref(), Some("cloneNode"));
        assert!(clone.args[0].optional);
        assert_eq!(clone.args[0].default, Some(ConstValue::Bool(false)));
        let Member::Operation(append) = &i.members[5] else {
            panic!()
        };
        assert!(append.args[0].variadic);
        assert!(matches!(append.args[0].ty.kind, TypeKind::Union(ref m) if m.len() == 2));
        let Member::Operation(item) = &i.members[6] else {
            panic!()
        };
        assert_eq!(item.special, Some(Special::Getter));
        assert!(item.ret.nullable);
        let Member::Operation(check) = &i.members[8] else {
            panic!()
        };
        assert!(check.is_static);
    }

    #[test]
    fn types_and_their_extended_attributes() {
        let i = interface(
            "interface T {
               undefined f([Clamp] octet a, [EnforceRange] unsigned long long b,
                 [LegacyNullToEmptyString] DOMString c, unrestricted double d,
                 sequence<record<ByteString, (long or Node)?>> e, Promise<any> f,
                 optional (A or B)? g = null);
             };",
        );
        let Member::Operation(f) = &i.members[0] else {
            panic!()
        };
        let a = &f.args[0].ty;
        assert_eq!(a.kind, TypeKind::Integer(IntegerType::Octet));
        assert!(a.ext_attrs.has("Clamp"));
        assert!(f.args[1].ty.ext_attrs.has("EnforceRange"));
        assert!(f.args[2].ty.ext_attrs.has("LegacyNullToEmptyString"));
        assert_eq!(
            f.args[3].ty.kind,
            TypeKind::Float(FloatType::UnrestrictedDouble)
        );
        let TypeKind::Sequence(inner) = &f.args[4].ty.kind else {
            panic!()
        };
        let TypeKind::Record(key, value) = &inner.kind else {
            panic!()
        };
        assert_eq!(key.kind, TypeKind::ByteString);
        assert!(value.nullable);
        assert!(matches!(f.args[5].ty.kind, TypeKind::Promise(_)));
        assert!(f.args[6].ty.nullable);
        assert_eq!(f.args[6].default, Some(ConstValue::Null));
    }

    #[test]
    fn dictionaries_enums_callbacks_typedefs_mixins() {
        let defs = parse(
            "t.idl",
            "dictionary D : Base { required long a; boolean b = true; sequence<long> c = []; };
             partial dictionary D { DOMString e; };
             enum E { \"one\", \"\", \"two-words\" };
             callback F = undefined (DOMString x, optional long y);
             callback interface L { undefined handleEvent(Event event); };
             typedef (long or DOMString) T;
             interface mixin M { attribute long m; };
             N includes M;",
        )
        .unwrap();
        assert_eq!(defs.len(), 8);
        let Definition::Dictionary(dict) = &defs[0] else {
            panic!()
        };
        assert!(dict.members[0].required);
        assert_eq!(dict.members[2].default, Some(ConstValue::EmptySequence));
        let Definition::Enum(enumeration) = &defs[2] else {
            panic!()
        };
        assert_eq!(enumeration.values, ["one", "", "two-words"]);
        let Definition::Interface(listener) = &defs[4] else {
            panic!()
        };
        assert!(listener.callback);
        let Definition::Interface(mixin) = &defs[6] else {
            panic!()
        };
        assert!(mixin.mixin);
        assert!(
            matches!(&defs[7], Definition::Includes(inc) if inc.target == "N" && inc.mixin == "M")
        );
    }

    #[test]
    fn extended_attribute_forms() {
        let i = interface(
            "[Exposed=(Window,Worker), Global=Window, LegacyFactoryFunction=Image(optional unsigned long width)]
             interface I {
               [Reflect=\"for\"] attribute DOMString htmlFor;
               [Reflect, ReflectRange=(0, 8), ReflectDefault=2] attribute unsigned long n;
               [Exposed=*, PutForwards=href] readonly attribute Location location;
             };",
        );
        let exposed = i.ext_attrs.get("Exposed").unwrap();
        assert_eq!(
            exposed.value,
            ExtValue::List(vec![
                ExtValue::Ident("Window".into()),
                ExtValue::Ident("Worker".into())
            ])
        );
        let factory = i.ext_attrs.get("LegacyFactoryFunction").unwrap();
        assert_eq!(factory.value, ExtValue::Ident("Image".into()));
        assert_eq!(factory.args.as_ref().unwrap().len(), 1);
        let Member::Attribute(n) = &i.members[1] else {
            panic!()
        };
        assert_eq!(
            n.ext_attrs.get("ReflectRange").unwrap().value,
            ExtValue::List(vec![ExtValue::Integer(0), ExtValue::Integer(8)])
        );
        let Member::Attribute(loc) = &i.members[2] else {
            panic!()
        };
        assert_eq!(
            loc.ext_attrs.get("Exposed").unwrap().value,
            ExtValue::Ident("*".into())
        );
    }

    #[test]
    fn errors_name_the_line() {
        let cases = [
            (
                "interface A {\n  attribute long;\n};",
                "t.idl:2: expected a name, found ';'",
            ),
            (
                "interface A {\n  long f(;\n};",
                "t.idl:2: expected a name, found ';'",
            ),
            (
                "interface A {\n",
                "t.idl:1: the interface 'A' is not closed",
            ),
            (
                "\n\nnamespace X {};",
                "t.idl:3: namespaces are not in the WebIDL subset of swb",
            ),
            (
                "interface A { readonly maplike<long, long>; };",
                "t.idl:1: 'maplike' is not in the WebIDL subset of swb",
            ),
            (
                "enum E { 1 };",
                "t.idl:1: expected a string in the enumeration 'E', found '1'",
            ),
            (
                "dictionary D {\n required long a = 1; };",
                "t.idl:2: the required member 'a' has a default value",
            ),
            (
                "interface A { const DOMString s = \"x\"; };",
                "t.idl:1: the constant 's' needs a boolean or a number",
            ),
            ("garbage", "t.idl:1: expected a definition, found 'garbage'"),
            (
                "interface A { attribute (long) x; };",
                "t.idl:1: a union type needs at least two member types",
            ),
        ];
        for (text, expected) in cases {
            let error = parse("t.idl", text).unwrap_err();
            assert_eq!(error.to_string(), expected, "{text}");
        }
    }

    #[test]
    fn keywords_as_argument_names() {
        let i = interface("interface N { DOMString? lookupPrefix(DOMString? namespace); };");
        let Member::Operation(op) = &i.members[0] else {
            panic!()
        };
        assert_eq!(op.args[0].name, "namespace");
    }
}
