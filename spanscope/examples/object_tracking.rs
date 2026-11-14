//! Tracks field writes and value changes of individual instances.
//!
//! Run with `cargo run -p spanscope --features object-tracking --example object_tracking`.

use spanscope::{trace, InstanceId, Tracked};

#[derive(Clone, Tracked)]
struct Item {
    #[tracked(id)]
    sku: u64,
    price: u64,
    quantity: u32,
}

#[derive(Clone, Tracked)]
struct Cart {
    #[tracked(id)]
    id: InstanceId,
    items: Vec<Item>,
    total: u64,
    #[tracked(skip)]
    scratch: Vec<u8>,
}

impl Cart {
    fn new(first_sku: u64, items: u64) -> Self {
        let items = (0..items)
            .map(|n| Item {
                sku: first_sku + n,
                price: 10 + n * 5,
                quantity: 1,
            })
            .collect::<Vec<_>>();
        let total = items.iter().map(|item| item.price).sum();
        Self {
            id: InstanceId::new(),
            items,
            total,
            scratch: Vec::new(),
        }
    }

    #[trace(track(self))]
    fn reprice(&mut self, sku: u64, price: u64) {
        for item in self.items.iter_mut().filter(|item| item.sku == sku) {
            item.price = price;
        }
        self.recompute();
        self.scratch.clear();
    }

    #[trace(track(self))]
    fn add_one(&mut self, sku: u64) {
        if let Some(item) = self.items.iter_mut().find(|item| item.sku == sku) {
            item.quantity += 1;
        }
    }

    #[trace(track(self))]
    fn apply_discount(&mut self, percent: u64) {
        for item in self.items.iter_mut() {
            item.price -= item.price * percent / 100;
        }
        self.recompute();
    }

    /// Writes `total` on every call even when nothing changed.
    #[trace(track(self))]
    fn recompute(&mut self) {
        self.total = self
            .items
            .iter()
            .map(|item| item.price * u64::from(item.quantity))
            .sum();
    }

    #[trace(track(self))]
    fn audit(&self) -> u64 {
        self.total
    }
}

#[trace(root)]
fn checkout(carts: &mut [Cart], round: u64) {
    for (position, cart) in carts.iter_mut().enumerate() {
        let first = cart.items[0].sku;
        if position % 2 == 0 {
            cart.reprice(first, 10 + round % 3);
        }
        if round % 4 == 0 {
            cart.add_one(first + 1);
        }
        cart.recompute();
        cart.audit();
    }
}

fn main() {
    spanscope::config::ConfigBuilder::new()
        .program("object_tracking")
        .output("target/spanscope-object-tracking.json")
        .init()
        .expect("spanscope configuration");

    let mut carts = (0..6)
        .map(|cart| Cart::new(100 * (cart + 1), 2 + cart % 3))
        .collect::<Vec<_>>();
    for round in 0..12 {
        checkout(&mut carts, round);
    }
    // A clone is a new instance that remembers where it came from; its items get new
    // SKUs so domain keys stay unique.
    let mut copy = carts[0].clone();
    copy.items.retain(|item| item.sku % 2 == 0);
    for item in copy.items.iter_mut() {
        item.sku += 1000;
    }
    copy.apply_discount(10);
    carts[1].apply_discount(25);

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
