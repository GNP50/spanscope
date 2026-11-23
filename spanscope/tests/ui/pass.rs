use spanscope::trace;

#[trace(root, name = "ui::free", tags("generic", "root"))]
fn free<T: Into<u32>>(input: T) -> impl Iterator<Item = u32> {
    [input.into()].into_iter()
}

struct Widget(u32);
impl Widget {
    #[trace]
    fn call(&self, offset: u32) -> u32 {
        self.0 + offset
    }
}

trait Trait {
    #[trace]
    fn default_method(&self) -> u32 {
        3
    }
}
impl Trait for Widget {}

fn main() {
    assert_eq!(free(2u8).sum::<u32>(), 2);
    assert_eq!(Widget(1).call(2), 3);
    assert_eq!(Widget(1).default_method(), 3);
}
