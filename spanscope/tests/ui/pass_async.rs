#[spanscope::trace]
async fn value() -> u32 {
    1
}

#[spanscope::trace(root, tags("async"))]
async fn borrowed<'a, T: AsRef<str> + Sync>(input: &'a T) -> Result<&'a str, ()> {
    let text = input.as_ref();
    if text.is_empty() { return Err(()); }
    Ok(text)
}

struct Worker(u32);

impl Worker {
    #[spanscope::trace]
    async fn compute(&self, increment: u32) -> impl Iterator<Item = u32> {
        [self.0 + increment].into_iter()
    }
}

trait DefaultWork {
    #[spanscope::trace]
    async fn run(&self) -> u32 { value().await }
}

impl DefaultWork for Worker {}

fn require_send<T: Send>(_: T) {}

fn main() {
    let worker = Worker(1);
    require_send(borrowed(&"input"));
    require_send(worker.compute(2));
    require_send(worker.run());
}
