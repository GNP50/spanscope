#[spanscope::trace(memo_probe)]
async fn fetch(value: u32) -> u32 {
    value
}

#[spanscope::trace(memo_probe(results))]
fn misspelled(value: u32) -> u32 {
    value
}

fn main() {}
