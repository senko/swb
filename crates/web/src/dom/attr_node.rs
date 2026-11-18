//! `Attr` as a `Node` (DOM, <https://dom.spec.whatwg.org/#interface-attr>
//! and <https://dom.spec.whatwg.org/#interface-node>).
//!
//! An `Attr` is a view of an attribute that the element keeps
//! ([`super::attrs`]); it is not a node of the arena. Its interface
//! inherits from `Node` (the IDL says `Attr : Node`, `[SwbNotANode]`), so
//! `Attr.prototype` has the `Node` members, and these functions are what
//! they do when an `Attr` is involved:
//!
//! - `this` is an `Attr`: the generated glue of a `Node` member finds no
//!   arena node and answers [`Exception::AttrNode`]; [`glue::run`] then
//!   calls [`node_member`], which acts as the DOM specifies for a node
//!   without a parent and without children.
//! - an argument is an `Attr` (`el.contains(attr)`, `el.appendChild(attr)`
//!   and the other members that take a `Node`): the conversion of the
//!   argument answers the same exception, and [`node_member`] answers for
//!   the members that Chromium accepts an `Attr` in; any other member
//!   fails as for a value that is not a node.
//!
//! Chromium's values come from the probe cases `attr-is-a-node` and
//! `bootstrap-sanitize-html` (`tools/probes/web-osm-gaps.json`). Chromium
//! places an attribute right after its element in document order for
//! `compareDocumentPosition`: the element contains it and precedes it, the
//! element's descendants and every later node follow it.
//!
//! [`glue::run`]: crate::bindings::glue::run

use swb_dom::NodeId;
use swb_js::{Gc, NativeCall, Object, Runtime, Value};

use super::attrs::{self, AttrData};
use crate::bindings::{self as b, Context, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom, engine_mut};

const DISCONNECTED: u16 = 0x01;
const PRECEDING: u16 = 0x02;
const FOLLOWING: u16 = 0x04;
const CONTAINS: u16 = 0x08;
const CONTAINED_BY: u16 = 0x10;
const IMPLEMENTATION_SPECIFIC: u16 = 0x20;

/// `Node` is the interface of the member that was called.
const NODE: &str = "Node";

/// The `Attr` of a call: `this`, or a node argument.
enum Subject {
    /// `this` is an `Attr`.
    This(Platform),
    /// `this` is a node and an argument is an `Attr`.
    Argument,
}

/// What a node argument of a member is.
#[derive(Clone, Copy)]
enum Arg {
    /// `null` (a nullable argument).
    Null,
    /// An `Attr`.
    Attr(Platform),
    /// A node of the current document or of another one.
    Node(b::NodeRef),
}

/// The step of `glue::run` for the exception [`Exception::AttrNode`]: the
/// member `context` of `Node`, with an `Attr` as `this` or as an argument.
pub(crate) fn node_member(
    rt: &mut Runtime,
    call: &NativeCall,
    context: Context,
) -> b::Result<Value> {
    let subject = if rt.implements(call.this(), g::ATTR) {
        Subject::This(b::glue::this_platform(rt, call, g::ATTR)?)
    } else {
        Subject::Argument
    };
    let answer = match (context, subject) {
        (Context::Getter(NODE, name), Subject::This(this)) => getter(rt, call, this, name),
        (Context::Setter(NODE, name), Subject::This(this)) => setter(rt, call, this, name),
        (Context::Operation(NODE, name), Subject::This(this)) => operation(rt, call, this, name),
        (Context::Operation(NODE, name), Subject::Argument) => attr_argument(rt, call, name),
        _ => None,
    };
    match answer {
        Some(answer) => answer,
        // Not a member that accepts an `Attr`: the value is not a node.
        None => Err(not_a_node(rt, call)),
    }
}

/// The exception of an `Attr` argument of a member that takes only nodes
/// of the arena: "parameter N is not of type 'Node'.".
fn not_a_node(rt: &Runtime, call: &NativeCall) -> Exception {
    let index = (0..call.argc())
        .find(|&i| rt.implements(rt.arg(call, i), g::ATTR))
        .unwrap_or(0);
    Exception::NotOfType("Node".into()).argument(index)
}

// ----- Attr as `this` -----

/// The getters of `Node` for an `Attr`.
fn getter(
    rt: &mut Runtime,
    call: &NativeCall,
    this: Platform,
    name: &str,
) -> Option<b::Result<Value>> {
    Some(match name {
        "nodeType" => Ok(Value::number(2.0)),
        "nodeName" => {
            let name = attrs::data_of(rt, this).map(|d| attrs::qualified_name(&d.name));
            string(rt, name)
        }
        "baseURI" => {
            let base = engine_mut(rt).map(crate::host::EngineHost::base_url);
            string(rt, base.map_err(Into::into))
        }
        "isConnected" => Ok(Value::Bool(false)),
        "ownerDocument" => owner_document(rt, call, this),
        "parentNode" | "parentElement" | "firstChild" | "lastChild" | "previousSibling"
        | "nextSibling" => Ok(Value::Null),
        "childNodes" => crate::dom::lists::new_static_list(rt, call.realm(), Vec::new())
            .map(Value::from)
            .map_err(Into::into),
        "nodeValue" | "textContent" => {
            let value = value_of(rt, call, this);
            string(rt, value)
        }
        _ => return None,
    })
}

/// A string result as a value.
fn string(rt: &mut Runtime, text: b::Result<String>) -> b::Result<Value> {
    Ok(b::to_js::string(rt, &text?)?)
}

/// The value of the attribute that `this` views.
fn value_of(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<String> {
    <WebState as g::attr::AttrImpl>::value(&mut b::Cx::new(rt, call, g::ATTR), this)
}

/// `Node.ownerDocument`: the node document of the owner element, or the
/// document in which a script made the `Attr`.
/// <https://dom.spec.whatwg.org/#dom-node-ownerdocument>
fn owner_document(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let data = attrs::data_of(rt, this)?;
    let (owner, made_by) = (data.owner, data.document_node);
    let document = match owner {
        Some(element) => dom(rt)?.node_document(element),
        None => made_by,
    };
    Ok(b::to_js::node::<WebState>(rt, call.realm(), document)?)
}

/// The setters of `Node` for an `Attr`: `nodeValue` and `textContent` set
/// the value (DOM "set an existing attribute value"; `null` is the empty
/// string).
/// <https://dom.spec.whatwg.org/#dom-node-nodevalue>
fn setter(
    rt: &mut Runtime,
    call: &NativeCall,
    this: Platform,
    name: &str,
) -> Option<b::Result<Value>> {
    if !matches!(name, "nodeValue" | "textContent") {
        return None;
    }
    let set = |rt: &mut Runtime| {
        let value = rt.arg(call, 0);
        let value = b::convert::dom_string_null_empty(rt, value).map_err(Exception::nested)?;
        b::ce::scope(rt, |rt| {
            <WebState as g::attr::AttrImpl>::set_value(
                &mut b::Cx::new(rt, call, g::ATTR),
                this,
                value,
            )
        })?;
        Ok(Value::Undefined)
    };
    Some(set(rt))
}

/// The number of arguments that the `Node` operation `name` requires (the
/// generated glue checks it after `this`, which an `Attr` fails first).
fn required_arguments(name: &str) -> usize {
    match name {
        "insertBefore" | "replaceChild" => 2,
        "isEqualNode"
        | "isSameNode"
        | "contains"
        | "compareDocumentPosition"
        | "lookupPrefix"
        | "lookupNamespaceURI"
        | "isDefaultNamespace"
        | "appendChild"
        | "removeChild" => 1,
        _ => 0,
    }
}

/// The operations of `Node` for an `Attr`.
fn operation(
    rt: &mut Runtime,
    call: &NativeCall,
    this: Platform,
    name: &str,
) -> Option<b::Result<Value>> {
    if let Err(error) = b::glue::require_arguments(call, required_arguments(name)) {
        return Some(Err(error));
    }
    Some(match name {
        "hasChildNodes" => Ok(Value::Bool(false)),
        "normalize" => Ok(Value::Undefined),
        // The root of a node without a parent is the node.
        "getRootNode" => Ok(Value::Object(this.object)),
        "cloneNode" => clone_node(rt, call, this),
        "isEqualNode" => is_equal_node(rt, call, this),
        "isSameNode" => arg(rt, call, 0, true).map(|other| {
            Value::Bool(matches!(other, Arg::Attr(other) if other.object == this.object))
        }),
        "contains" => arg(rt, call, 0, true).map(|other| {
            Value::Bool(matches!(other, Arg::Attr(other) if other.object == this.object))
        }),
        "compareDocumentPosition" => compare_position(rt, call, this),
        "lookupPrefix" => lookup_prefix(rt, call, this),
        "lookupNamespaceURI" => lookup_namespace(rt, call, this),
        "isDefaultNamespace" => is_default_namespace(rt, call, this),
        "insertBefore" | "appendChild" | "replaceChild" | "removeChild" => {
            refuse_tree_change(rt, call, name)
        }
        _ => return None,
    })
}

/// A node argument, as the IDL converts it (`nullable`: `null` is allowed).
fn arg(rt: &Runtime, call: &NativeCall, index: usize, nullable: bool) -> b::Result<Arg> {
    let value = rt.arg(call, index);
    if nullable && matches!(value, Value::Null | Value::Undefined) {
        return Ok(Arg::Null);
    }
    if rt.implements(value, g::ATTR) {
        return Ok(Arg::Attr(b::convert::platform(rt, value, g::ATTR, NODE)?));
    }
    b::convert::node_ref(rt, value, g::NODE, NODE)
        .map(Arg::Node)
        .map_err(|e| e.argument(index))
}

/// `appendChild`, `insertBefore`, `replaceChild` and `removeChild` of an
/// `Attr`: the DOM "ensure pre-insertion validity" refuses a parent that
/// is not a document, a fragment or an element (Chromium words it as the
/// node type not supporting the method; `removeChild` throws
/// `NotFoundError`). The arguments are converted first, as the IDL says.
/// <https://dom.spec.whatwg.org/#concept-node-ensure-pre-insertion-validity>
fn refuse_tree_change(rt: &Runtime, call: &NativeCall, name: &str) -> b::Result<Value> {
    let count = if matches!(name, "insertBefore" | "replaceChild") {
        2
    } else {
        1
    };
    for index in 0..count {
        arg(rt, call, index, name == "insertBefore" && index == 1)?;
    }
    let message = "This node type does not support this method.";
    Err(if name == "removeChild" {
        Exception::dom("NotFoundError", message)
    } else {
        Exception::dom("HierarchyRequestError", message)
    })
}

/// `cloneNode`: a new `Attr` with the namespace, prefix, local name and
/// value (DOM "clone a node", the `Attr` case).
/// <https://dom.spec.whatwg.org/#concept-node-clone>
fn clone_node(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let data = attrs::data_of(rt, this)?;
    let (name, document) = (data.name.clone(), data.document_node);
    let value = value_of(rt, call, this)?;
    let copy: Gc<Object> = b::ce::scope(rt, |rt| {
        let copy = attrs::new_attr(rt, call.realm(), None, name, value)?;
        attrs::set_document_node(rt, copy, document)?;
        Ok(copy)
    })?;
    Ok(Value::Object(copy))
}

/// `isEqualNode`: two `Attr`s are equal if their namespaces, local names
/// and values are.
/// <https://dom.spec.whatwg.org/#concept-node-equals>
fn is_equal_node(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let Arg::Attr(other) = arg(rt, call, 0, true)? else {
        return Ok(Value::Bool(false));
    };
    let key = |data: &AttrData| (data.name.ns.clone(), data.name.local.clone());
    let same_name = key(attrs::data_of(rt, this)?) == key(attrs::data_of(rt, other)?);
    let equal = same_name && value_of(rt, call, this)? == value_of(rt, call, other)?;
    Ok(Value::Bool(equal))
}

/// `compareDocumentPosition` of an `Attr` and a node or another `Attr`.
/// <https://dom.spec.whatwg.org/#dom-node-comparedocumentposition>
fn compare_position(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let other = arg(rt, call, 0, false)?;
    let mine = attrs::data_of(rt, this)?.owner;
    let bits = match other {
        Arg::Null => 0,
        Arg::Attr(other) if other.object == this.object => 0,
        Arg::Attr(other) => {
            let theirs = attrs::data_of(rt, other)?.owner;
            attr_to_attr(rt, mine, theirs, this, other)?
        }
        Arg::Node(node) => match (mine, node.here(rt)) {
            (Some(element), Some(node)) => inverse(attr_rank(rt, node, element)?),
            _ => DISCONNECTED | IMPLEMENTATION_SPECIFIC | FOLLOWING,
        },
    };
    Ok(Value::number(f64::from(bits)))
}

/// The position of `node` for the attribute of `element`, from `node`'s
/// point of view: `compareDocumentPosition(attr)` of `node`.
fn attr_rank(rt: &Runtime, node: NodeId, element: NodeId) -> b::Result<u16> {
    if node == element {
        return Ok(CONTAINED_BY | FOLLOWING);
    }
    let bits = dom(rt)?.compare_position(node, element);
    if bits & DISCONNECTED != 0 {
        return Ok(bits);
    }
    // A descendant of the element comes after its attributes.
    Ok(if bits & CONTAINS != 0 {
        PRECEDING
    } else {
        bits
    })
}

/// The position seen from the other side: the bits of
/// `compareDocumentPosition` swap their directions.
fn inverse(bits: u16) -> u16 {
    let mut swapped = bits & (DISCONNECTED | IMPLEMENTATION_SPECIFIC);
    for (from, to) in [
        (PRECEDING, FOLLOWING),
        (FOLLOWING, PRECEDING),
        (CONTAINS, CONTAINED_BY),
        (CONTAINED_BY, CONTAINS),
    ] {
        if bits & from != 0 {
            swapped |= to;
        }
    }
    swapped
}

/// Two different attributes (DOM `compareDocumentPosition`, the steps for
/// `attr1` and `attr2`): those of one element follow the order of the
/// attribute list (implementation specific); those of two elements of one
/// tree follow the order of the elements (an ancestor's attributes precede
/// the descendant's, with no containment bit); the others are disconnected,
/// in an order that stays the same while both live (the slot of the
/// object, as the DOM orders disconnected nodes by creation).
fn attr_to_attr(
    rt: &Runtime,
    mine: Option<NodeId>,
    theirs: Option<NodeId>,
    this: Platform,
    other: Platform,
) -> b::Result<u16> {
    if let (Some(a), Some(b)) = (mine, theirs)
        && a != b
    {
        let bits = dom(rt)?.compare_position(a, b);
        if bits & DISCONNECTED != 0 {
            return Ok(bits);
        }
        return Ok(if bits & PRECEDING != 0 {
            PRECEDING
        } else {
            FOLLOWING
        });
    }
    if let (Some(a), Some(b)) = (mine, theirs)
        && a == b
    {
        let position = |attr: Platform| -> b::Result<Option<usize>> {
            let data = attrs::data_of(rt, attr)?;
            Ok(dom(rt)?.element(a).and_then(|e| {
                e.attributes()
                    .iter()
                    .position(|x| x.name.ns == data.name.ns && x.name.local == data.name.local)
            }))
        };
        let order = if position(this)? <= position(other)? {
            FOLLOWING
        } else {
            PRECEDING
        };
        return Ok(IMPLEMENTATION_SPECIFIC | order);
    }
    let order = if other.object.index() < this.object.index() {
        PRECEDING
    } else {
        FOLLOWING
    };
    Ok(DISCONNECTED | IMPLEMENTATION_SPECIFIC | order)
}

/// The element that the namespace lookups of an `Attr` start at: its owner.
fn owner_of(rt: &Runtime, this: Platform) -> b::Result<Option<NodeId>> {
    Ok(attrs::data_of(rt, this)?.owner)
}

/// `lookupPrefix` of an `Attr`: DOM "locate a namespace prefix" of the
/// owner element, or null without one.
/// <https://dom.spec.whatwg.org/#dom-node-lookupprefix>
fn lookup_prefix(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let value = rt.arg(call, 0);
    let namespace = nullable_string(rt, value)?.filter(|n| !n.is_empty());
    let found = match (owner_of(rt, this)?, namespace) {
        (Some(element), Some(namespace)) => super::locate_prefix(dom(rt)?, element, &namespace),
        _ => None,
    };
    nullable(rt, found)
}

/// `lookupNamespaceURI` of an `Attr`.
/// <https://dom.spec.whatwg.org/#dom-node-lookupnamespaceuri>
fn lookup_namespace(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let value = rt.arg(call, 0);
    let prefix = nullable_string(rt, value)?.filter(|p| !p.is_empty());
    let found = match owner_of(rt, this)? {
        Some(element) => super::locate_namespace(dom(rt)?, element, prefix.as_deref()),
        None => None,
    };
    nullable(rt, found)
}

/// `isDefaultNamespace` of an `Attr`.
/// <https://dom.spec.whatwg.org/#dom-node-isdefaultnamespace>
fn is_default_namespace(rt: &mut Runtime, call: &NativeCall, this: Platform) -> b::Result<Value> {
    let value = rt.arg(call, 0);
    let namespace = nullable_string(rt, value)?.filter(|n| !n.is_empty());
    let default = match owner_of(rt, this)? {
        Some(element) => super::locate_namespace(dom(rt)?, element, None),
        None => None,
    };
    Ok(Value::Bool(default == namespace))
}

/// A `DOMString?` argument.
fn nullable_string(rt: &mut Runtime, value: Value) -> b::Result<Option<String>> {
    if value == Value::Null {
        return Ok(None);
    }
    Ok(Some(
        b::convert::dom_string(rt, value).map_err(|e| e.argument(0))?,
    ))
}

/// A `DOMString?` result.
fn nullable(rt: &mut Runtime, text: Option<String>) -> b::Result<Value> {
    match text {
        Some(text) => Ok(b::to_js::string(rt, &text)?),
        None => Ok(Value::Null),
    }
}

// ----- An Attr as an argument -----

/// A member of `Node` of a node that gets an `Attr` where it takes a node.
fn attr_argument(rt: &mut Runtime, call: &NativeCall, name: &str) -> Option<b::Result<Value>> {
    let attr_at = |index: usize| rt.implements(rt.arg(call, index), g::ATTR);
    let this = b::glue::this_node(rt, call, g::NODE).ok()?;
    Some(match name {
        "isEqualNode" | "isSameNode" | "contains" => Ok(Value::Bool(false)),
        "compareDocumentPosition" => match b::convert::platform(rt, rt.arg(call, 0), g::ATTR, NODE)
        {
            Ok(attr) => attr_position(rt, this, attr).map(|bits| Value::number(f64::from(bits))),
            Err(error) => Err(error.argument(0)),
        },
        "appendChild" | "insertBefore" | "replaceChild" if attr_at(0) => {
            let child = attr_name(rt, rt.arg(call, 0));
            Err(hierarchy_error(rt, this, &child))
        }
        "insertBefore" if attr_at(1) => Err(Exception::dom(
            "NotFoundError",
            "The node before which the new node is to be inserted is not a child of this node.",
        )),
        "replaceChild" if attr_at(1) => Err(Exception::dom(
            "NotFoundError",
            "The node to be replaced is not a child of this node.",
        )),
        "removeChild" if attr_at(0) => Err(Exception::dom(
            "NotFoundError",
            "The node to be removed is not a child of this node.",
        )),
        _ => return None,
    })
}

/// Chromium's `HierarchyRequestError` for an `Attr` inserted into `parent`.
fn hierarchy_error(rt: &Runtime, parent: NodeId, child: &str) -> Exception {
    let parent = dom(rt).map_or_else(
        |_| String::new(),
        |dom| super::node_name(&dom.node(parent).data),
    );
    Exception::dom(
        "HierarchyRequestError",
        format!("Nodes of type '{child}' may not be inserted inside nodes of type '{parent}'."),
    )
}

/// The `nodeName` of the `Attr` that `value` is.
fn attr_name(rt: &Runtime, value: Value) -> String {
    b::convert::platform(rt, value, g::ATTR, NODE)
        .ok()
        .and_then(|attr| attrs::data_of(rt, attr).ok())
        .map(|d| attrs::qualified_name(&d.name))
        .unwrap_or_default()
}

/// `node.compareDocumentPosition(attr)`.
fn attr_position(rt: &Runtime, this: NodeId, attr: Platform) -> b::Result<u16> {
    let Some(element) = attrs::data_of(rt, attr)?.owner else {
        return Ok(DISCONNECTED | IMPLEMENTATION_SPECIFIC | PRECEDING);
    };
    attr_rank(rt, this, element)
}
