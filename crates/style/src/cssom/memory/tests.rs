use super::*;

#[test]
fn holds_return_their_bytes() {
    let memory = StyleMemory::with_limit(100);
    let mut hold = memory.reserve(60, "test").unwrap();
    assert_eq!(memory.used(), 60);
    assert_eq!(memory.free(), 40);
    assert!(memory.reserve(41, "test").is_none());
    // A split keeps the total.
    let part = hold.split(25);
    assert_eq!((hold.bytes(), part.bytes()), (35, 25));
    assert_eq!(memory.used(), 60);
    drop(part);
    assert_eq!(memory.used(), 35);
    // A split of more than is left takes what is left.
    let rest = hold.split(100);
    assert_eq!((hold.bytes(), rest.bytes()), (0, 35));
    drop(rest);
    drop(hold);
    assert_eq!(memory.used(), 0);
}

#[test]
fn resize_replaces_the_old_bytes() {
    let memory = StyleMemory::with_limit(100);
    let mut hold = None;
    memory.resize(&mut hold, 70);
    assert_eq!(memory.used(), 70);
    memory.resize(&mut hold, 30);
    assert_eq!(memory.used(), 30);
    drop(hold);
    assert_eq!(memory.used(), 0);
}

#[test]
fn budgets_count_what_is_free() {
    let memory = StyleMemory::with_limit(100);
    let _hold = memory.reserve(60, "test").unwrap();
    assert!(memory.budget(0).allows(40));
    assert!(!memory.budget(0).allows(41));
    // Bytes that the rule has not been charged for yet come first.
    assert!(!memory.budget(10).allows(31));
    assert!(WriteBudget::unlimited().allows(usize::MAX));
}

#[test]
fn values_cost_their_tokens() {
    let tokens = swb_css::parse_component_values("1 1 1 1");
    // Four numbers and three spaces, each a component value.
    assert_eq!(values_bytes(&tokens), 7 * size_of::<ComponentValue>());
    let tokens = swb_css::parse_component_values("rgb(1, 2, 3)");
    assert!(values_bytes(&tokens) > 8 * size_of::<ComponentValue>());
}
