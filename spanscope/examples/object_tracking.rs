//! Tracks field writes and value changes of individual instances.
//!
//! Run with `cargo run -p spanscope --features object-tracking --example object_tracking`.

use spanscope::{trace, InstanceId, Tracked};

#[derive(Tracked)]
struct Item {
    #[tracked(id)]
    sku: u64,
    price: u64,
}

#[derive(Tracked)]
struct Cart {
    #[tracked(id)]
    id: InstanceId,
    items: Vec<Item>,
    total: u64,
    #[tracked(skip)]
    scratch: Vec<u8>,
}

impl Cart {
    #[trace(track(self))]
    fn reprice(&mut self, sku: u64, price: u64) {
        for item in self.items.iter_mut().filter(|item| item.sku == sku) {
            item.price = price;
        }
        self.total = self.items.iter().map(|item| item.price).sum();
        self.scratch.clear();
    }

    #[trace(track(self))]
    fn audit(&self) -> u64 {
        self.total
    }
}

fn main() {
    spanscope::config::ConfigBuilder::new()
        .program("object_tracking")
        .output("target/spanscope-object-tracking.json")
        .init()
        .expect("spanscope configuration");

    let mut first = Cart {
        id: InstanceId::new(),
        items: vec![Item { sku: 1, price: 10 }, Item { sku: 2, price: 20 }],
        total: 30,
        scratch: Vec::new(),
    };
    let mut second = Cart {
        id: InstanceId::new(),
        items: vec![Item { sku: 3, price: 5 }],
        total: 5,
        scratch: Vec::new(),
    };
    for round in 0..10 {
        first.reprice(1, 10 + round % 3);
        second.reprice(3, 5);
        first.audit();
    }

    let snapshot = spanscope::collection::snapshot();
    for instance in &snapshot.objects.instances {
        println!(
            "{} {}: {} versions, field changes {:?}",
            instance.object.type_name,
            instance.object.key,
            instance.versions,
            instance.field_changes
        );
    }
    let path = spanscope::export::flush().expect("profile export");
    println!("profile written to {}", path.display());
}
