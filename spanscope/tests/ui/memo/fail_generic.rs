#[spanscope::memo]
fn identity<T: Clone>(value: T) -> T {
    value
}

fn main() {}
