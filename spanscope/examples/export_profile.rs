//! Generate a real profile: cargo run -p spanscope --features enabled --example export_profile -- --spanscope-output viewer/fixtures/example.json

#[spanscope::trace(root, name = "demo::workload", tags("example"))]
fn workload(size: u64) -> u64 {
    let mut total = 0;
    for value in 0..size {
        total += transform(value);
    }
    spanscope::metric!("input_size", size);
    total
}

#[spanscope::trace(name = "demo::transform")]
fn transform(value: u64) -> u64 {
    value.wrapping_mul(31).rotate_left(7)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let remaining = spanscope::config::init_from_args(std::env::args())?;
    for _ in 0..4 {
        std::hint::black_box(workload(200));
    }
    let child = spanscope::context::propagate();
    std::thread::spawn(move || {
        let _attached = child.attach();
        std::hint::black_box(workload(80));
    })
    .join()
    .unwrap();
    if remaining.iter().any(|argument| argument == "--explicit") {
        spanscope::export::flush()?;
    }
    Ok(())
}
