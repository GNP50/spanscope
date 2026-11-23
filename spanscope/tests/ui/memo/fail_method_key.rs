struct Engine;

impl Engine {
    #[spanscope::memo]
    fn value(&self, input: u32) -> u32 {
        input
    }
}

fn main() {}
