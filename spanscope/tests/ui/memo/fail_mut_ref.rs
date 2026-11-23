#[spanscope::memo]
fn push(values: &mut Vec<u32>, value: u32) -> usize {
    values.push(value);
    values.len()
}

fn main() {}
