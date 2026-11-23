#[spanscope::memo]
fn total(values: impl Iterator<Item = u32>) -> u32 {
    values.sum()
}

fn main() {}
