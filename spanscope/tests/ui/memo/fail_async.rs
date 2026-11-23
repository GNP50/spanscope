#[spanscope::memo]
async fn fetch(value: u32) -> u32 {
    value
}

fn main() {}
