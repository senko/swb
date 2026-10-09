//! Properties, property descriptors and `ValidateAndApplyPropertyDescriptor`
//! (ECMA-262 §6.1.7.1, §6.2.6, §10.1.6.3).

use crate::error::{Error, Result};
use crate::heap::Heap;
use crate::value::Value;

/// An own property: a data property or an accessor property with all its
/// attributes (§6.1.7.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Property {
    /// A data property.
    Data {
        /// `[[Value]]`.
        value: Value,
        /// `[[Writable]]`.
        writable: bool,
        /// `[[Enumerable]]`.
        enumerable: bool,
        /// `[[Configurable]]`.
        configurable: bool,
    },
    /// An accessor property. The getter and the setter are functions or
    /// `undefined`.
    Accessor {
        /// `[[Get]]`.
        get: Value,
        /// `[[Set]]`.
        set: Value,
        /// `[[Enumerable]]`.
        enumerable: bool,
        /// `[[Configurable]]`.
        configurable: bool,
    },
}

impl Property {
    /// A writable, enumerable, configurable data property (the kind that
    /// `CreateDataProperty` makes).
    pub const fn data(value: Value) -> Self {
        Property::Data {
            value,
            writable: true,
            enumerable: true,
            configurable: true,
        }
    }

    /// Whether this is a writable, enumerable, configurable data property.
    pub const fn is_default_data(&self) -> bool {
        matches!(
            self,
            Property::Data {
                writable: true,
                enumerable: true,
                configurable: true,
                ..
            }
        )
    }

    /// `[[Enumerable]]`.
    pub const fn enumerable(&self) -> bool {
        match *self {
            Property::Data { enumerable, .. } | Property::Accessor { enumerable, .. } => enumerable,
        }
    }

    /// `[[Configurable]]`.
    pub const fn configurable(&self) -> bool {
        match *self {
            Property::Data { configurable, .. } | Property::Accessor { configurable, .. } => {
                configurable
            }
        }
    }

    /// Whether this is an accessor property.
    pub const fn is_accessor(&self) -> bool {
        matches!(self, Property::Accessor { .. })
    }

    /// The attribute flags.
    pub(crate) fn flags(&self) -> Flags {
        match *self {
            Property::Data {
                writable,
                enumerable,
                configurable,
                ..
            } => Flags::data(writable, enumerable, configurable),
            Property::Accessor {
                enumerable,
                configurable,
                ..
            } => Flags::accessor_property(enumerable, configurable),
        }
    }

    /// The values of the property's slots: the value, or the getter and
    /// the setter.
    pub(crate) fn slot_values(&self) -> (Value, Option<Value>) {
        match *self {
            Property::Data { value, .. } => (value, None),
            Property::Accessor { get, set, .. } => (get, Some(set)),
        }
    }

    /// The property with these flags and slot values.
    pub(crate) fn from_flags(flags: Flags, first: Value, second: Value) -> Self {
        if flags.accessor() {
            Property::Accessor {
                get: first,
                set: second,
                enumerable: flags.enumerable(),
                configurable: flags.configurable(),
            }
        } else {
            Property::Data {
                value: first,
                writable: flags.writable(),
                enumerable: flags.enumerable(),
                configurable: flags.configurable(),
            }
        }
    }
}

/// The attributes of a stored property, as bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Flags(u8);

impl Flags {
    const WRITABLE: u8 = 1;
    const ENUMERABLE: u8 = 2;
    const CONFIGURABLE: u8 = 4;
    const ACCESSOR: u8 = 8;
    /// A removed entry of a dictionary (a tombstone).
    const DELETED: u8 = 16;

    /// The flags of a data property.
    pub(crate) const fn data(writable: bool, enumerable: bool, configurable: bool) -> Self {
        Flags(bit(writable, Self::WRITABLE) | Self::common(enumerable, configurable))
    }

    /// The flags of an accessor property.
    pub(crate) const fn accessor_property(enumerable: bool, configurable: bool) -> Self {
        Flags(Self::ACCESSOR | Self::common(enumerable, configurable))
    }

    const fn common(enumerable: bool, configurable: bool) -> u8 {
        bit(enumerable, Self::ENUMERABLE) | bit(configurable, Self::CONFIGURABLE)
    }

    pub(crate) const fn writable(self) -> bool {
        self.0 & Self::WRITABLE != 0
    }

    pub(crate) const fn enumerable(self) -> bool {
        self.0 & Self::ENUMERABLE != 0
    }

    pub(crate) const fn configurable(self) -> bool {
        self.0 & Self::CONFIGURABLE != 0
    }

    pub(crate) const fn accessor(self) -> bool {
        self.0 & Self::ACCESSOR != 0
    }

    pub(crate) const fn deleted(self) -> bool {
        self.0 & Self::DELETED != 0
    }

    pub(crate) const fn with_deleted(self) -> Self {
        Flags(self.0 | Self::DELETED)
    }

    /// The number of value slots: two for an accessor, one otherwise.
    pub(crate) const fn width(self) -> u32 {
        if self.accessor() { 2 } else { 1 }
    }
}

/// `mask` if `set`, otherwise 0.
const fn bit(set: bool, mask: u8) -> u8 {
    if set { mask } else { 0 }
}

/// A property descriptor (§6.2.6): each field may be absent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PropertyDescriptor {
    /// `[[Value]]`.
    pub value: Option<Value>,
    /// `[[Writable]]`.
    pub writable: Option<bool>,
    /// `[[Get]]`.
    pub get: Option<Value>,
    /// `[[Set]]`.
    pub set: Option<Value>,
    /// `[[Enumerable]]`.
    pub enumerable: Option<bool>,
    /// `[[Configurable]]`.
    pub configurable: Option<bool>,
}

impl PropertyDescriptor {
    /// A complete data descriptor.
    pub const fn data(value: Value, writable: bool, enumerable: bool, configurable: bool) -> Self {
        PropertyDescriptor {
            value: Some(value),
            writable: Some(writable),
            get: None,
            set: None,
            enumerable: Some(enumerable),
            configurable: Some(configurable),
        }
    }

    /// A descriptor with only `[[Value]]` (as `OrdinarySet` uses it).
    pub const fn value(value: Value) -> Self {
        PropertyDescriptor {
            value: Some(value),
            writable: None,
            get: None,
            set: None,
            enumerable: None,
            configurable: None,
        }
    }

    /// A complete accessor descriptor.
    pub const fn accessor(get: Value, set: Value, enumerable: bool, configurable: bool) -> Self {
        PropertyDescriptor {
            value: None,
            writable: None,
            get: Some(get),
            set: Some(set),
            enumerable: Some(enumerable),
            configurable: Some(configurable),
        }
    }

    /// `IsAccessorDescriptor` (§6.2.6.1).
    pub const fn is_accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }

    /// `IsDataDescriptor` (§6.2.6.2).
    pub const fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }

    /// `IsGenericDescriptor` (§6.2.6.3).
    pub const fn is_generic(&self) -> bool {
        !self.is_accessor() && !self.is_data()
    }

    /// Whether the descriptor has no fields.
    pub const fn is_empty(&self) -> bool {
        self.is_generic() && self.enumerable.is_none() && self.configurable.is_none()
    }

    /// Checks what `ToPropertyDescriptor` (§6.2.6.5) guarantees: not both
    /// kinds of field, and no internal `Empty` value.
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_accessor() && self.is_data() {
            return Err(Error::type_error(
                "Invalid property descriptor. Cannot both specify accessors and a value or writable attribute",
            ));
        }
        let fields = [self.value, self.get, self.set];
        if fields.iter().flatten().any(|value| value.is_internal()) {
            return Err(Error::invariant("an Empty value in a property descriptor"));
        }
        Ok(())
    }
}

/// What `ValidateAndApplyPropertyDescriptor` decides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Apply {
    /// Return false.
    Reject,
    /// Return true; nothing changes.
    Unchanged,
    /// Create this property (there was none).
    Create(Property),
    /// Replace the current property with this one.
    Update(Property),
}

/// `ValidateAndApplyPropertyDescriptor` (§10.1.6.3), without the writes:
/// it returns the decision, and the caller applies it to its storage.
pub(crate) fn validate_and_apply(
    heap: &Heap,
    extensible: bool,
    desc: &PropertyDescriptor,
    current: Option<Property>,
) -> Result<Apply> {
    let undefined = Value::Undefined;
    // Step 2: no current property.
    let Some(current) = current else {
        if !extensible {
            return Ok(Apply::Reject);
        }
        let enumerable = desc.enumerable.unwrap_or(false);
        let configurable = desc.configurable.unwrap_or(false);
        let property = if desc.is_accessor() {
            Property::Accessor {
                get: desc.get.unwrap_or(undefined),
                set: desc.set.unwrap_or(undefined),
                enumerable,
                configurable,
            }
        } else {
            Property::Data {
                value: desc.value.unwrap_or(undefined),
                writable: desc.writable.unwrap_or(false),
                enumerable,
                configurable,
            }
        };
        return Ok(Apply::Create(property));
    };
    // Step 4.
    if desc.is_empty() {
        return Ok(Apply::Unchanged);
    }
    // Step 5: a non-configurable property allows only some changes.
    if !current.configurable() {
        if desc.configurable == Some(true) {
            return Ok(Apply::Reject);
        }
        if desc.enumerable.is_some_and(|e| e != current.enumerable()) {
            return Ok(Apply::Reject);
        }
        if !desc.is_generic() && desc.is_accessor() != current.is_accessor() {
            return Ok(Apply::Reject);
        }
        match current {
            Property::Accessor { get, set, .. } => {
                if let Some(new_get) = desc.get
                    && !heap.same_value(new_get, get)?
                {
                    return Ok(Apply::Reject);
                }
                if let Some(new_set) = desc.set
                    && !heap.same_value(new_set, set)?
                {
                    return Ok(Apply::Reject);
                }
            }
            Property::Data {
                value,
                writable: false,
                ..
            } => {
                if desc.writable == Some(true) {
                    return Ok(Apply::Reject);
                }
                if let Some(new_value) = desc.value
                    && !heap.same_value(new_value, value)?
                {
                    return Ok(Apply::Reject);
                }
            }
            Property::Data { .. } => {}
        }
    }
    // Step 6: the new property.
    let enumerable = desc.enumerable.unwrap_or(current.enumerable());
    let configurable = desc.configurable.unwrap_or(current.configurable());
    let property = match current {
        Property::Data { .. } if desc.is_accessor() => Property::Accessor {
            get: desc.get.unwrap_or(undefined),
            set: desc.set.unwrap_or(undefined),
            enumerable,
            configurable,
        },
        Property::Accessor { .. } if desc.is_data() => Property::Data {
            value: desc.value.unwrap_or(undefined),
            writable: desc.writable.unwrap_or(false),
            enumerable,
            configurable,
        },
        Property::Data {
            value, writable, ..
        } => Property::Data {
            value: desc.value.unwrap_or(value),
            writable: desc.writable.unwrap_or(writable),
            enumerable,
            configurable,
        },
        Property::Accessor { get, set, .. } => Property::Accessor {
            get: desc.get.unwrap_or(get),
            set: desc.set.unwrap_or(set),
            enumerable,
            configurable,
        },
    };
    Ok(Apply::Update(property))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::HeapConfig;

    fn run(desc: PropertyDescriptor, current: Option<Property>, extensible: bool) -> Apply {
        let heap = Heap::new(HeapConfig::default());
        validate_and_apply(&heap, extensible, &desc, current).unwrap()
    }

    const FROZEN: Property = Property::Data {
        value: Value::Int(1),
        writable: false,
        enumerable: true,
        configurable: false,
    };

    #[test]
    fn creation_fills_defaults() {
        assert_eq!(
            run(PropertyDescriptor::value(Value::Int(5)), None, true),
            Apply::Create(Property::Data {
                value: Value::Int(5),
                writable: false,
                enumerable: false,
                configurable: false
            })
        );
        let accessor = PropertyDescriptor {
            set: Some(Value::Null),
            ..Default::default()
        };
        assert_eq!(
            run(accessor, None, true),
            Apply::Create(Property::Accessor {
                get: Value::Undefined,
                set: Value::Null,
                enumerable: false,
                configurable: false
            })
        );
        assert_eq!(
            run(PropertyDescriptor::value(Value::Int(5)), None, false),
            Apply::Reject
        );
    }

    #[test]
    fn non_configurable_rules() {
        let current = Some(FROZEN);
        // Same value: allowed (no change).
        assert!(matches!(
            run(PropertyDescriptor::value(Value::Double(1.0)), current, true),
            Apply::Update(_)
        ));
        // Another value, writable true, configurable true, enumerable
        // change, kind change: rejected.
        for desc in [
            PropertyDescriptor::value(Value::Int(2)),
            PropertyDescriptor {
                writable: Some(true),
                ..Default::default()
            },
            PropertyDescriptor {
                configurable: Some(true),
                ..Default::default()
            },
            PropertyDescriptor {
                enumerable: Some(false),
                ..Default::default()
            },
            PropertyDescriptor {
                get: Some(Value::Undefined),
                ..Default::default()
            },
        ] {
            assert_eq!(run(desc, current, true), Apply::Reject, "{desc:?}");
        }
        assert_eq!(
            run(PropertyDescriptor::default(), current, true),
            Apply::Unchanged
        );
        // -0 and +0 differ for SameValue.
        let zero = Property::Data {
            value: Value::Int(0),
            writable: false,
            enumerable: false,
            configurable: false,
        };
        assert_eq!(
            run(
                PropertyDescriptor::value(Value::Double(-0.0)),
                Some(zero),
                true
            ),
            Apply::Reject
        );
        // NaN equals NaN for SameValue.
        let nan = Property::Data {
            value: Value::Double(f64::NAN),
            writable: false,
            enumerable: false,
            configurable: false,
        };
        assert!(matches!(
            run(
                PropertyDescriptor::value(Value::Double(f64::NAN)),
                Some(nan),
                true
            ),
            Apply::Update(_)
        ));
    }

    #[test]
    fn writable_non_configurable_can_change_value_and_become_read_only() {
        let current = Some(Property::Data {
            value: Value::Int(1),
            writable: true,
            enumerable: false,
            configurable: false,
        });
        assert_eq!(
            run(
                PropertyDescriptor {
                    value: Some(Value::Int(2)),
                    writable: Some(false),
                    ..Default::default()
                },
                current,
                true
            ),
            Apply::Update(Property::Data {
                value: Value::Int(2),
                writable: false,
                enumerable: false,
                configurable: false
            })
        );
    }

    #[test]
    fn non_configurable_accessor_keeps_its_functions() {
        let current = Some(Property::Accessor {
            get: Value::Null,
            set: Value::Undefined,
            enumerable: false,
            configurable: false,
        });
        let same = PropertyDescriptor {
            get: Some(Value::Null),
            ..Default::default()
        };
        assert!(matches!(run(same, current, true), Apply::Update(_)));
        let other = PropertyDescriptor {
            set: Some(Value::Null),
            ..Default::default()
        };
        assert_eq!(run(other, current, true), Apply::Reject);
    }

    #[test]
    fn configurable_kind_changes_keep_attributes() {
        let current = Some(Property::Data {
            value: Value::Int(1),
            writable: true,
            enumerable: true,
            configurable: true,
        });
        let to_accessor = PropertyDescriptor {
            get: Some(Value::Null),
            ..Default::default()
        };
        assert_eq!(
            run(to_accessor, current, true),
            Apply::Update(Property::Accessor {
                get: Value::Null,
                set: Value::Undefined,
                enumerable: true,
                configurable: true
            })
        );
        let accessor = Some(Property::Accessor {
            get: Value::Null,
            set: Value::Null,
            enumerable: false,
            configurable: true,
        });
        let to_data = PropertyDescriptor {
            writable: Some(true),
            ..Default::default()
        };
        assert_eq!(
            run(to_data, accessor, true),
            Apply::Update(Property::Data {
                value: Value::Undefined,
                writable: true,
                enumerable: false,
                configurable: true
            })
        );
    }

    #[test]
    fn descriptor_check() {
        let both = PropertyDescriptor {
            value: Some(Value::Int(1)),
            get: Some(Value::Undefined),
            ..Default::default()
        };
        assert!(matches!(both.check(), Err(Error::Throw { .. })));
        assert!(
            PropertyDescriptor::value(Value::Empty)
                .check()
                .is_err_and(|e| matches!(e, Error::Internal(_)))
        );
    }
}
