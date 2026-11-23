//! Object tracking compiles in every feature set and records instances when enabled.
#![allow(dead_code)]

use spanscope::{trace, InstanceId, Tracked};
use std::cell::RefCell;

#[derive(Tracked)]
struct Counter {
    #[tracked(id)]
    id: InstanceId,
    hits: u64,
    label: String,
    #[tracked(skip)]
    cache: Vec<u8>,
}

impl Counter {
    fn new(label: &str) -> Self {
        Self {
            id: InstanceId::new(),
            hits: 0,
            label: label.to_owned(),
            cache: Vec::new(),
        }
    }
}

#[trace(track(counter))]
fn bump(counter: &mut Counter, by: u64) -> u64 {
    if by == 0 {
        return counter.hits;
    }
    counter.hits += by;
    let borrowed = &mut counter.cache;
    borrowed.push(1);
    counter.label.len() as u64
}

#[trace(track(counter))]
fn rename(counter: &mut Counter, label: &str) -> Result<(), String> {
    let parsed: u32 = label.parse().map_err(|_| "not a number".to_owned())?;
    counter.label = parsed.to_string();
    Ok(())
}

#[derive(Tracked)]
struct Line {
    #[tracked(id)]
    sku: u32,
    quantity: u32,
}

#[derive(Tracked)]
struct Order {
    #[tracked(id)]
    number: String,
    lines: Vec<Line>,
}

#[trace(track(order))]
fn restock(order: &mut Order, sku: u32) {
    for line in order.lines.iter_mut() {
        if line.sku == sku {
            line.quantity += 1;
        }
    }
}

#[derive(Tracked)]
struct Shared {
    #[tracked(id)]
    id: InstanceId,
    value: RefCell<u32>,
}

#[trace(track(shared))]
fn poke(shared: &Shared) {
    *shared.value.borrow_mut() += 1;
}

#[derive(Tracked)]
struct Anonymous {
    value: u8,
}

#[trace(track(item))]
fn flip(item: &mut Anonymous) {
    item.value ^= 1;
}

#[derive(Tracked)]
struct Slot {
    #[tracked(id)]
    id: InstanceId,
    value: u8,
}

#[trace(track(slot))]
fn replace(slot: &mut Slot) {
    *slot = Slot {
        id: InstanceId::new(),
        value: slot.value,
    };
}

#[derive(Tracked)]
enum Phase {
    Idle,
    Running { step: u32 },
}

#[derive(Tracked)]
struct Machine {
    #[tracked(id)]
    id: InstanceId,
    phase: Phase,
}

impl Machine {
    #[trace(track(self))]
    fn advance(&mut self) {
        self.phase = match self.phase {
            Phase::Idle => Phase::Running { step: 1 },
            Phase::Running { step } => Phase::Running { step: step + 1 },
        };
    }
}

#[derive(Tracked)]
struct Job {
    #[tracked(id)]
    id: InstanceId,
    done: bool,
}

#[trace(track(job))]
async fn finish(job: &mut Job) -> bool {
    std::future::ready(()).await;
    job.done = true;
    job.done
}

#[test]
fn instance_ids_do_not_affect_equality() {
    let id = InstanceId::new();
    assert_eq!(id, id.clone());
    assert!(format!("{:?}", InstanceId::new()).starts_with("InstanceId"));
}

#[test]
fn tracked_functions_keep_their_behavior() {
    let mut counter = Counter::new("abc");
    assert_eq!(bump(&mut counter, 0), 0);
    assert_eq!(bump(&mut counter, 2), 3);
    assert_eq!(counter.hits, 2);
    assert!(rename(&mut counter, "x").is_err());
    assert!(rename(&mut counter, "7").is_ok());
    assert_eq!(counter.label, "7");
    let mut machine = Machine {
        id: InstanceId::new(),
        phase: Phase::Idle,
    };
    machine.advance();
    assert!(matches!(machine.phase, Phase::Running { step: 1 }));
    let mut job = Job {
        id: InstanceId::new(),
        done: false,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(runtime.block_on(finish(&mut job)));
}

#[cfg(feature = "object-tracking")]
mod enabled {
    use super::*;
    use spanscope::collection::snapshot;
    use spanscope::objects::{Access, InstanceKey, InstanceSnapshot, ObjectSnapshot};

    fn instance<'a>(
        objects: &'a ObjectSnapshot,
        ty: &str,
        key: &InstanceKey,
    ) -> &'a InstanceSnapshot {
        objects
            .instances
            .iter()
            .find(|instance| instance.object.type_name.ends_with(ty) && instance.object.key == *key)
            .unwrap_or_else(|| panic!("missing {ty} {key}"))
    }

    fn generated(id: &InstanceId) -> InstanceKey {
        InstanceKey::Generated(id.value().unwrap())
    }

    #[test]
    fn counts_calls_changes_and_sites_per_instance() {
        let mut counter = Counter::new("abc");
        let untouched = Counter::new("other");
        bump(&mut counter, 0);
        bump(&mut counter, 1);
        bump(&mut counter, 1);
        let key = generated(&counter.id);
        drop(untouched);
        let objects = snapshot().objects;
        let record = instance(&objects, "::Counter", &key);
        assert!(!record.approximate);
        assert_eq!(record.versions, 2);
        assert_eq!(record.field_changes.get("hits"), Some(&2));
        assert!(!record.field_changes.contains_key("cache"), "skipped field");
        assert_eq!(record.calls.len(), 1);
        assert_eq!(record.calls[0].calls, 3);
        assert_eq!(record.calls[0].changed_calls, 2);
        assert_eq!(record.history.len(), 2);
        assert_eq!(record.history[1].version, 2);
        assert_eq!(record.history[1].changed, ["hits"]);
        let accesses = objects
            .sites
            .iter()
            .filter(|site| site.type_name.ends_with("::Counter"))
            .map(|site| (site.path, site.access))
            .collect::<Vec<_>>();
        assert!(accesses.contains(&("hits", Access::Write)), "{accesses:?}");
        assert!(accesses.contains(&("hits", Access::Read)), "{accesses:?}");
        assert!(accesses.contains(&("cache", Access::Write)), "{accesses:?}");
        assert!(accesses.contains(&("label", Access::Call)), "{accesses:?}");
        let write_line = objects
            .sites
            .iter()
            .find(|site| {
                site.type_name.ends_with("::Counter")
                    && site.path == "hits"
                    && site.access == Access::Write
            })
            .unwrap()
            .line;
        let expected = include_str!("object_tracking.rs")
            .lines()
            .position(|line| line.trim() == "counter.hits += by;")
            .unwrap()
            + 1;
        assert_eq!(
            write_line as usize, expected,
            "line of `counter.hits += by`"
        );
    }

    #[test]
    fn early_returns_and_question_mark_are_recorded() {
        let mut counter = Counter::new("abc");
        assert!(rename(&mut counter, "nope").is_err());
        rename(&mut counter, "42").unwrap();
        let objects = snapshot().objects;
        let record = instance(&objects, "::Counter", &generated(&counter.id));
        let calls = record.calls.iter().map(|calls| calls.calls).sum::<u64>();
        assert_eq!(calls, 2);
        assert_eq!(record.versions, 1);
        assert_eq!(record.field_changes.get("label"), Some(&1));
    }

    #[test]
    fn nested_instances_receive_their_own_changes() {
        let mut order = Order {
            number: "A-17".into(),
            lines: vec![
                Line {
                    sku: 1,
                    quantity: 0,
                },
                Line {
                    sku: 2,
                    quantity: 0,
                },
            ],
        };
        restock(&mut order, 2);
        restock(&mut order, 2);
        let objects = snapshot().objects;
        let parent = instance(&objects, "::Order", &InstanceKey::Domain("A-17".into()));
        assert_eq!(parent.versions, 2);
        assert_eq!(parent.field_changes.get("lines[*].quantity"), Some(&2));
        let line = instance(&objects, "::Line", &InstanceKey::Domain("2".into()));
        assert_eq!(line.versions, 2);
        assert_eq!(line.field_changes.get("quantity"), Some(&2));
        assert_eq!(line.parent.as_ref(), Some(&parent.object));
        assert!(
            !objects
                .instances
                .iter()
                .any(|instance| instance.object.key == InstanceKey::Domain("1".into())),
            "unchanged line"
        );
    }

    #[test]
    fn interior_mutability_is_found_by_hashing() {
        let shared = Shared {
            id: InstanceId::new(),
            value: RefCell::new(0),
        };
        poke(&shared);
        let objects = snapshot().objects;
        let record = instance(&objects, "::Shared", &generated(&shared.id));
        assert_eq!(record.versions, 1);
        assert_eq!(record.field_changes.get("value"), Some(&1));
    }

    #[test]
    fn instances_without_ids_fall_back_to_addresses() {
        let mut item = Anonymous { value: 0 };
        flip(&mut item);
        let address = &item as *const Anonymous as usize;
        let objects = snapshot().objects;
        let record = instance(&objects, "::Anonymous", &InstanceKey::Address(address));
        assert!(record.approximate);
        assert_eq!(record.versions, 1);
    }

    #[test]
    fn clones_and_replacements_are_linked() {
        let original = Counter::new("origin");
        let mut copy = Counter {
            id: original.id.clone(),
            hits: original.hits,
            label: original.label.clone(),
            cache: Vec::new(),
        };
        bump(&mut copy, 1);
        let mut slot = Slot {
            id: InstanceId::new(),
            value: 3,
        };
        let before = generated(&slot.id);
        replace(&mut slot);
        let after = generated(&slot.id);
        let objects = snapshot().objects;
        let cloned = instance(&objects, "::Counter", &generated(&copy.id));
        assert_eq!(cloned.cloned_from, original.id.value());
        let replaced = instance(&objects, "::Slot", &before);
        assert_eq!(
            replaced.replaced_by.as_ref().map(|object| &object.key),
            Some(&after)
        );
    }

    #[test]
    fn enum_variants_and_methods_on_self() {
        let mut machine = Machine {
            id: InstanceId::new(),
            phase: Phase::Idle,
        };
        machine.advance();
        machine.advance();
        let objects = snapshot().objects;
        let record = instance(&objects, "::Machine", &generated(&machine.id));
        assert_eq!(record.versions, 2);
        assert_eq!(record.field_changes.get("phase"), Some(&2));
    }

    #[test]
    fn async_functions_are_tracked() {
        let mut job = Job {
            id: InstanceId::new(),
            done: false,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(finish(&mut job));
        let objects = snapshot().objects;
        let record = instance(&objects, "::Job", &generated(&job.id));
        assert_eq!(record.versions, 1);
        assert_eq!(record.field_changes.get("done"), Some(&1));
    }
}
