//! Legacy platform objects (Web IDL §3.9) and iterable declarations
//! (§3.7.9) of the test IDL, with a test implementation: `TestList`
//! (`NodeList`), `TestCollection` (`HTMLCollection`), `TestOptionList`
//! (`HTMLOptionsCollection`: an indexed setter and inherited properties),
//! `TestStringMap` (`DOMStringMap`), `TestStorage` (`Storage`), `TestForm`
//! (`HTMLFormElement`, a node) and `TestHeaders` (`Headers`, a pair
//! iterator). The expected values come from Chromium 148 with the real
//! interfaces (`tools/probes/web-legacy.json`); where Chromium departs
//! from Web IDL, the cases follow Web IDL and say so.

mod cases;

use swb_js::{
    GenericData, HostData, NativeCall, NativeReturn, Runtime, Tracer, Value, VmError, VmResult,
};
use swb_web::bindings::{self as b, Cx, Exception, Host, NodeId, Platform};

use super::{TestHost, generated, host, new_node, node_mut};

/// The nodes of a list or collection.
struct Items(Vec<NodeId>);

impl GenericData for Items {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<NodeId>()
    }
}

/// The entries of a string map, a storage area or headers, in order.
struct Entries(Vec<(String, String)>);

impl GenericData for Entries {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.0.iter().map(|(k, v)| k.len() + v.len()).sum()
    }
}

fn items(rt: &mut Runtime, this: Platform) -> b::Result<&mut Vec<NodeId>> {
    let HostData::Generic(data) = this.data else {
        return Err(Exception::type_error("not a list"));
    };
    Ok(&mut rt
        .heap_mut()
        .generic_mut::<Items>(data)
        .map_err(VmError::from)?
        .0)
}

fn entries(rt: &mut Runtime, this: Platform) -> b::Result<&mut Vec<(String, String)>> {
    let HostData::Generic(data) = this.data else {
        return Err(Exception::type_error("not a map"));
    };
    Ok(&mut rt
        .heap_mut()
        .generic_mut::<Entries>(data)
        .map_err(VmError::from)?
        .0)
}

fn length_of(len: usize) -> b::Result<u32> {
    u32::try_from(len).map_err(|_| Exception::range_error("too long"))
}

/// The value of an attribute of a fake element.
fn attribute(rt: &mut Runtime, node: NodeId, name: &str) -> b::Result<Option<String>> {
    Ok(node_mut(rt, node)?
        .attributes
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone()))
}

/// The supported property names of a collection (DOM §4.2.10.2): the `id`
/// and then the `name` of each element, without duplicates.
fn collection_names(rt: &mut Runtime, nodes: &[NodeId]) -> b::Result<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    for &node in nodes {
        for which in ["id", "name"] {
            if let Some(name) = attribute(rt, node, which)?
                && !name.is_empty()
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
    }
    Ok(names)
}

/// The first element whose `id` or `name` is `name`.
fn named_element(rt: &mut Runtime, nodes: &[NodeId], name: &str) -> b::Result<Option<NodeId>> {
    for &node in nodes {
        for which in ["id", "name"] {
            if attribute(rt, node, which)?.as_deref() == Some(name) {
                return Ok(Some(node));
            }
        }
    }
    Ok(None)
}

impl generated::test_list::TestListImpl for TestHost {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        length_of(items(cx.rt, this)?.len())
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        Ok(items(cx.rt, this)?.get(index as usize).copied())
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        length_of(items(cx.rt, this)?.len())
    }
}

impl generated::test_collection::TestCollectionImpl for TestHost {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        length_of(items(cx.rt, this)?.len())
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<NodeId>> {
        Ok(items(cx.rt, this)?.get(index as usize).copied())
    }

    fn named_item(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Option<NodeId>> {
        let nodes = items(cx.rt, this)?.clone();
        named_element(cx.rt, &nodes, &name)
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        length_of(items(cx.rt, this)?.len())
    }

    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        let nodes = items(cx.rt, this)?.clone();
        collection_names(cx.rt, &nodes)
    }
}

impl generated::test_option_list::TestOptionListImpl for TestHost {
    /// HTML's "set the value of a new indexed property or of an existing
    /// one" of `HTMLOptionsCollection`, cut down: `null` removes, a gap is
    /// filled with new elements.
    fn indexed_setter(
        cx: &mut Cx<'_>,
        this: Platform,
        index: u32,
        value: Option<NodeId>,
    ) -> b::Result<()> {
        let index = index as usize;
        let Some(node) = value else {
            let list = items(cx.rt, this)?;
            if index < list.len() {
                list.remove(index);
            }
            return Ok(());
        };
        while items(cx.rt, this)?.len() < index {
            let filler = new_node(cx.rt, "o", true)?;
            items(cx.rt, this)?.push(filler);
        }
        let list = items(cx.rt, this)?;
        match list.get_mut(index) {
            Some(slot) => *slot = node,
            None => list.push(node),
        }
        Ok(())
    }
}

impl generated::test_string_map::TestStringMapImpl for TestHost {
    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        Ok(entries(cx.rt, this)?
            .iter()
            .map(|(k, _)| k.clone())
            .collect())
    }

    fn named_getter(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<String> {
        Ok(entries(cx.rt, this)?
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default())
    }

    /// `DOMStringMap`'s setter: a `-` before a lowercase letter is a
    /// `SyntaxError`.
    fn named_setter(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        let bytes = name.as_bytes();
        if bytes
            .windows(2)
            .any(|w| w[0] == b'-' && w[1].is_ascii_lowercase())
        {
            return Err(Exception::dom(
                "SyntaxError",
                format!("'{name}' is not a valid property name."),
            ));
        }
        let list = entries(cx.rt, this)?;
        match list.iter_mut().find(|(k, _)| *k == name) {
            Some(entry) => entry.1 = value,
            None => list.push((name, value)),
        }
        Ok(())
    }

    fn named_deleter(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<()> {
        entries(cx.rt, this)?.retain(|(k, _)| *k != name);
        Ok(())
    }
}

impl generated::test_storage::TestStorageImpl for TestHost {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        length_of(entries(cx.rt, this)?.len())
    }

    fn key(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<String>> {
        Ok(entries(cx.rt, this)?
            .get(index as usize)
            .map(|(k, _)| k.clone()))
    }

    fn get_item(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<Option<String>> {
        Ok(entries(cx.rt, this)?
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone()))
    }

    fn set_item(cx: &mut Cx<'_>, this: Platform, key: String, value: String) -> b::Result<()> {
        let list = entries(cx.rt, this)?;
        match list.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => list.push((key, value)),
        }
        Ok(())
    }

    fn remove_item(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<()> {
        entries(cx.rt, this)?.retain(|(k, _)| *k != key);
        Ok(())
    }

    fn clear(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        entries(cx.rt, this)?.clear();
        Ok(())
    }

    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        Ok(entries(cx.rt, this)?
            .iter()
            .map(|(k, _)| k.clone())
            .collect())
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        Ok(entries(cx.rt, this)?.iter().any(|(k, _)| k == name))
    }
}

/// The elements of a form.
fn form_elements(rt: &mut Runtime, form: NodeId) -> b::Result<Vec<NodeId>> {
    Ok(node_mut(rt, form)?.children.clone())
}

impl generated::test_form::TestFormImpl for TestHost {
    fn length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        length_of(form_elements(cx.rt, this)?.len())
    }

    fn index_count(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        length_of(form_elements(cx.rt, this)?.len())
    }

    fn indexed_getter(cx: &mut Cx<'_>, this: NodeId, index: u32) -> b::Result<NodeId> {
        form_elements(cx.rt, this)?
            .get(index as usize)
            .copied()
            .ok_or_else(|| Exception::type_error("no element"))
    }

    fn supported_names(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Vec<String>> {
        let nodes = form_elements(cx.rt, this)?;
        collection_names(cx.rt, &nodes)
    }

    fn named_getter(cx: &mut Cx<'_>, this: NodeId, name: String) -> b::Result<NodeId> {
        let nodes = form_elements(cx.rt, this)?;
        named_element(cx.rt, &nodes, &name)?.ok_or_else(|| Exception::type_error("no element"))
    }
}

impl generated::test_headers::TestHeadersImpl for TestHost {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let data = cx.rt.alloc_host_data(Entries(Vec::new()))?;
        Ok(cx.new_instance::<TestHost>(data)?)
    }

    fn append(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        entries(cx.rt, this)?.push((name.to_ascii_lowercase(), value));
        Ok(())
    }

    fn delete(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<()> {
        let name = name.to_ascii_lowercase();
        entries(cx.rt, this)?.retain(|(k, _)| *k != name);
        Ok(())
    }

    /// The pairs sorted by name (as `Headers` sorts them).
    fn pair_at(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<(String, String)>> {
        let mut pairs = entries(cx.rt, this)?.clone();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(pairs.get(index as usize).cloned())
    }
}

// --- Natives that make the objects ---

/// The node of an argument (a wrapper).
fn node_arg(rt: &Runtime, call: &NativeCall, index: usize) -> VmResult<NodeId> {
    rt.host_object(rt.arg(call, index))
        .and_then(|(_, data)| b::node_of(data))
        .ok_or(VmError::type_error("not a node"))
}

/// A new object of `class` with the nodes of the arguments.
fn make_items(
    rt: &mut Runtime,
    call: &NativeCall,
    class: swb_js::ClassId,
) -> VmResult<NativeReturn> {
    let nodes = (0..call.argc())
        .map(|i| node_arg(rt, call, i))
        .collect::<VmResult<Vec<_>>>()?;
    let data = rt.alloc_host_data(Items(nodes))?;
    let object = rt.new_host_object(call.realm(), class, data)?;
    Ok(NativeReturn::Value(object.into()))
}

/// `makeList(...nodes)`.
fn make_list(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    make_items(rt, call, generated::TEST_LIST)
}

/// `makeCollection(...elements)`.
fn make_collection(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    make_items(rt, call, generated::TEST_COLLECTION)
}

/// `makeOptionList(...elements)`.
fn make_option_list(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    make_items(rt, call, generated::TEST_OPTION_LIST)
}

/// `listPush(list, node)`: the list or collection changes under the
/// script.
fn list_push(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let node = node_arg(rt, call, 1)?;
    let Value::Object(object) = rt.arg(call, 0) else {
        return Err(VmError::type_error("not a list"));
    };
    let (_, data) = rt
        .host_object(object.into())
        .ok_or(VmError::type_error("not a list"))?;
    items(rt, Platform { object, data })
        .map_err(|_| VmError::type_error("not a list"))?
        .push(node);
    Ok(NativeReturn::Value(Value::Undefined))
}

/// A new object of `class` with no entries.
fn make_entries(
    rt: &mut Runtime,
    call: &NativeCall,
    class: swb_js::ClassId,
) -> VmResult<NativeReturn> {
    let data = rt.alloc_host_data(Entries(Vec::new()))?;
    let object = rt.new_host_object(call.realm(), class, data)?;
    Ok(NativeReturn::Value(object.into()))
}

/// `makeStringMap()`.
fn make_string_map(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    make_entries(rt, call, generated::TEST_STRING_MAP)
}

/// `makeStorage()`.
fn make_storage(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    make_entries(rt, call, generated::TEST_STORAGE)
}

/// `makeForm()`: a `TestForm` node.
fn make_form(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let id = new_node(rt, "form", true).map_err(|_| VmError::type_error("node"))?;
    host(rt)
        .map_err(|_| VmError::type_error("no host"))?
        .nodes
        .get_mut(id.index())
        .ok_or(VmError::type_error("no node"))?
        .form = true;
    Ok(NativeReturn::Value(
        TestHost::wrap_node(rt, call.realm(), id)?.into(),
    ))
}

/// `formAdd(form, element)`.
fn form_add(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let form = node_arg(rt, call, 0)?;
    let element = node_arg(rt, call, 1)?;
    node_mut(rt, form)
        .map_err(|_| VmError::type_error("no node"))?
        .children
        .push(element);
    Ok(NativeReturn::Value(Value::Undefined))
}

/// The natives of these tests (registered by `runtime`).
pub(super) const FUNCTIONS: &[(&str, u32, swb_js::NativeFn)] = &[
    ("makeList", 0, make_list),
    ("makeCollection", 0, make_collection),
    ("makeOptionList", 0, make_option_list),
    ("listPush", 2, list_push),
    ("makeStringMap", 0, make_string_map),
    ("makeStorage", 0, make_storage),
    ("makeForm", 0, make_form),
    ("formAdd", 2, form_add),
];
