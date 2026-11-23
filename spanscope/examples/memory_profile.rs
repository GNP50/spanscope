//! Generate a profile with heap balance data:
//! cargo run -p spanscope --features alloc-tracker --example memory_profile -- --spanscope-output viewer/fixtures/memory.json

use std::collections::HashMap;

#[global_allocator]
static ALLOC: spanscope::TrackingAllocator<std::alloc::System> =
    spanscope::TrackingAllocator::new(std::alloc::System);

/// One request: parse, transform, render, and cache the result.
#[spanscope::trace(root, name = "demo::request", tags("example", "memory"))]
fn request(cache: &mut HashMap<u64, Vec<u8>>, id: u64, size: usize) -> usize {
    spanscope::attribute!(
        "request.size",
        if size >= 4_000 { "large" } else { "small" }
    );
    let records = parse(size);
    let rendered = render(&records);
    let packed = compress(&rendered);
    store(cache, id, packed);
    spanscope::outcome!("ok");
    rendered.len()
}

/// Builds owned records; the vector is returned, so its memory is retained.
#[spanscope::trace(name = "demo::parse")]
fn parse(size: usize) -> Vec<String> {
    (0..size / 40).map(|n| format!("record-{n:08}")).collect()
}

/// Joins records through many short-lived strings (allocation churn).
#[spanscope::trace(name = "demo::render")]
fn render(records: &[String]) -> String {
    let mut out = String::new();
    for record in records {
        let line = format!("<li>{}</li>", record.to_uppercase());
        out.push_str(&line);
    }
    out
}

/// Uses a large scratch buffer that is freed before returning (transient peak).
#[spanscope::trace(name = "demo::compress")]
fn compress(input: &str) -> Vec<u8> {
    let mut scratch = vec![0u8; input.len() * 8 + 1_048_576];
    for (slot, byte) in scratch.iter_mut().zip(input.bytes()) {
        *slot = byte ^ 0x5a;
    }
    scratch.iter().step_by(16).copied().collect()
}

/// Inserts into a cache that is never evicted (retention).
#[spanscope::trace(name = "demo::store")]
fn store(cache: &mut HashMap<u64, Vec<u8>>, id: u64, packed: Vec<u8>) {
    cache.insert(id, packed);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let remaining = spanscope::config::init_from_args(std::env::args())?;
    let mut cache = HashMap::new();
    for id in 0..24u64 {
        let size = if id % 6 == 5 { 16_000 } else { 2_000 };
        std::hint::black_box(request(&mut cache, id, size));
    }
    std::hint::black_box(&cache);
    if remaining.iter().any(|argument| argument == "--explicit") {
        spanscope::export::flush()?;
    }
    Ok(())
}
