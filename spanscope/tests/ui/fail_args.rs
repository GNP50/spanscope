#[spanscope::trace(name = 42)]
fn value() -> u32 {
    1
}

fn main() {}
