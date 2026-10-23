//! The syntax tree of the Web IDL subset that swb uses (ADR 0028
//! section 5; <https://webidl.spec.whatwg.org/#idl-grammar>).

/// A definition of a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Definition {
    /// An interface, a partial interface, an interface mixin or a callback
    /// interface.
    Interface(Interface),
    /// A dictionary or a partial dictionary.
    Dictionary(Dictionary),
    /// An enumeration.
    Enum(Enum),
    /// A callback function.
    Callback(CallbackFunction),
    /// A typedef.
    Typedef(Typedef),
    /// `A includes B;`.
    Includes(Includes),
}

/// The place of a definition or member: the file and the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Place {
    /// The file name.
    pub file: String,
    /// The line (1-based).
    pub line: u32,
}

/// An extended attribute (`[Name]`, `[Name=value]`, `[Name(args)]`,
/// `[Name=Ident(args)]`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtAttr {
    /// The name.
    pub name: String,
    /// The value after `=`.
    pub value: ExtValue,
    /// The arguments in parentheses (`[LegacyFactoryFunction=Image(...)]`).
    pub args: Option<Vec<Argument>>,
    /// Where it is.
    pub place: Place,
}

/// The value of an extended attribute.
#[derive(Clone, Debug, PartialEq)]
pub enum ExtValue {
    /// No value.
    None,
    /// An identifier (`[PutForwards=href]`) or `*` (`[Exposed=*]`).
    Ident(String),
    /// A string (`[Reflect="for"]`).
    String(String),
    /// An integer (`[ReflectDefault=1]`).
    Integer(i64),
    /// A decimal (`[ReflectDefault=1.5]`).
    Decimal(String),
    /// A parenthesized list (`[Exposed=(Window,Worker)]`,
    /// `[ReflectRange=(0, 8)]`).
    List(Vec<ExtValue>),
}

/// The extended attributes of a construct.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtAttrs(pub Vec<ExtAttr>);

impl ExtAttrs {
    /// The attribute named `name`.
    pub fn get(&self, name: &str) -> Option<&ExtAttr> {
        self.0.iter().find(|a| a.name == name)
    }

    /// Whether an attribute named `name` is present.
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
}

/// An interface, mixin or callback interface.
#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    /// The name.
    pub name: String,
    /// The inherited interface.
    pub inherits: Option<String>,
    /// `partial interface`.
    pub partial: bool,
    /// `interface mixin`.
    pub mixin: bool,
    /// `callback interface`.
    pub callback: bool,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// The members in order.
    pub members: Vec<Member>,
    /// Where it is.
    pub place: Place,
}

/// A member of an interface.
#[derive(Clone, Debug, PartialEq)]
pub enum Member {
    /// A constant.
    Const(Const),
    /// An attribute.
    Attribute(Attribute),
    /// An operation (also a special operation).
    Operation(Operation),
    /// A constructor operation (`constructor(...)`).
    Constructor(Operation),
    /// `stringifier;`.
    Stringifier(Place),
    /// `iterable<V>` or `iterable<K, V>`.
    Iterable(Iterable),
}

/// A constant.
#[derive(Clone, Debug, PartialEq)]
pub struct Const {
    /// The name.
    pub name: String,
    /// The type.
    pub ty: Type,
    /// The value.
    pub value: ConstValue,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// Where it is.
    pub place: Place,
}

/// The value of a constant or a default value.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstValue {
    /// `true` or `false`.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A decimal as written, or `Infinity`, `-Infinity`, `NaN`.
    Float(String),
    /// A string (defaults only).
    String(String),
    /// `null` (defaults only).
    Null,
    /// `{}` (defaults only).
    EmptyDictionary,
    /// `[]` (defaults only).
    EmptySequence,
    /// `undefined` (defaults only).
    Undefined,
}

/// An attribute.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    /// The name.
    pub name: String,
    /// The type.
    pub ty: Type,
    /// `readonly`.
    pub readonly: bool,
    /// `static`.
    pub is_static: bool,
    /// `stringifier attribute`.
    pub stringifier: bool,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// Where it is.
    pub place: Place,
}

/// A special keyword of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    /// `getter`.
    Getter,
    /// `setter`.
    Setter,
    /// `deleter`.
    Deleter,
}

/// An operation.
#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    /// The name (none for some special operations and constructors).
    pub name: Option<String>,
    /// The return type (`undefined` for constructors).
    pub ret: Type,
    /// The arguments.
    pub args: Vec<Argument>,
    /// `getter`, `setter` or `deleter`.
    pub special: Option<Special>,
    /// `static`.
    pub is_static: bool,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// Where it is.
    pub place: Place,
}

/// An argument of an operation or callback.
#[derive(Clone, Debug, PartialEq)]
pub struct Argument {
    /// The name.
    pub name: String,
    /// The type (with its extended attributes).
    pub ty: Type,
    /// `optional`.
    pub optional: bool,
    /// `...`.
    pub variadic: bool,
    /// The default value.
    pub default: Option<ConstValue>,
    /// The extended attributes of the argument.
    pub ext_attrs: ExtAttrs,
}

/// `iterable<...>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Iterable {
    /// The key type of a pair iterator.
    pub key: Option<Type>,
    /// The value type.
    pub value: Type,
    /// Where it is.
    pub place: Place,
}

/// A type.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    /// What type it is.
    pub kind: TypeKind,
    /// `T?`.
    pub nullable: bool,
    /// The extended attributes of the type (`[Clamp]`, `[EnforceRange]`,
    /// `[LegacyNullToEmptyString]`).
    pub ext_attrs: ExtAttrs,
}

/// The kinds of types.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    /// `any`.
    Any,
    /// `undefined`.
    Undefined,
    /// `boolean`.
    Boolean,
    /// An integer type.
    Integer(IntegerType),
    /// `float`, `unrestricted float`, `double`, `unrestricted double`.
    Float(FloatType),
    /// `bigint`.
    BigInt,
    /// `DOMString`.
    DomString,
    /// `ByteString`.
    ByteString,
    /// `USVString`.
    UsvString,
    /// `object`.
    Object,
    /// `symbol`.
    Symbol,
    /// An interface, dictionary, enumeration, callback or typedef name.
    Named(String),
    /// `sequence<T>`.
    Sequence(Box<Type>),
    /// `FrozenArray<T>`.
    FrozenArray(Box<Type>),
    /// `record<K, V>`.
    Record(Box<Type>, Box<Type>),
    /// `Promise<T>`.
    Promise(Box<Type>),
    /// `(A or B ...)`.
    Union(Vec<Type>),
}

/// The integer types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerType {
    /// `byte`.
    Byte,
    /// `octet`.
    Octet,
    /// `short`.
    Short,
    /// `unsigned short`.
    UnsignedShort,
    /// `long`.
    Long,
    /// `unsigned long`.
    UnsignedLong,
    /// `long long`.
    LongLong,
    /// `unsigned long long`.
    UnsignedLongLong,
}

/// The floating-point types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatType {
    /// `float`.
    Float,
    /// `unrestricted float`.
    UnrestrictedFloat,
    /// `double`.
    Double,
    /// `unrestricted double`.
    UnrestrictedDouble,
}

/// A dictionary.
#[derive(Clone, Debug, PartialEq)]
pub struct Dictionary {
    /// The name.
    pub name: String,
    /// The inherited dictionary.
    pub inherits: Option<String>,
    /// `partial dictionary`.
    pub partial: bool,
    /// The members in declaration order.
    pub members: Vec<DictionaryMember>,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// Where it is.
    pub place: Place,
}

/// A member of a dictionary.
#[derive(Clone, Debug, PartialEq)]
pub struct DictionaryMember {
    /// The name.
    pub name: String,
    /// The type.
    pub ty: Type,
    /// `required`.
    pub required: bool,
    /// The default value.
    pub default: Option<ConstValue>,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// Where it is.
    pub place: Place,
}

/// An enumeration.
#[derive(Clone, Debug, PartialEq)]
pub struct Enum {
    /// The name.
    pub name: String,
    /// The values in order.
    pub values: Vec<String>,
    /// Where it is.
    pub place: Place,
}

/// A callback function.
#[derive(Clone, Debug, PartialEq)]
pub struct CallbackFunction {
    /// The name.
    pub name: String,
    /// The return type.
    pub ret: Type,
    /// The arguments.
    pub args: Vec<Argument>,
    /// The extended attributes.
    pub ext_attrs: ExtAttrs,
    /// Where it is.
    pub place: Place,
}

/// A typedef.
#[derive(Clone, Debug, PartialEq)]
pub struct Typedef {
    /// The name.
    pub name: String,
    /// The type.
    pub ty: Type,
    /// Where it is.
    pub place: Place,
}

/// `target includes mixin;`.
#[derive(Clone, Debug, PartialEq)]
pub struct Includes {
    /// The interface.
    pub target: String,
    /// The mixin.
    pub mixin: String,
    /// Where it is.
    pub place: Place,
}
