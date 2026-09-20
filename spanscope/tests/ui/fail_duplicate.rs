#[spanscope::trace(root, root)]
fn value() -> u32 {
    1
}

fn main() {}
