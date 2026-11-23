#[spanscope::memo(max_entries = 4, max_bytes = 1024)]
fn both(value: u32) -> u32 {
    value
}

#[spanscope::memo(on_full = evict)]
fn policy(value: u32) -> u32 {
    value
}

#[spanscope::memo(lifetime = 3)]
fn unknown(value: u32) -> u32 {
    value
}

fn main() {}
