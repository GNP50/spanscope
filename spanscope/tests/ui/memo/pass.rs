use spanscope::memo::Cache;

#[spanscope::memo(key = (scale), max_entries = 4, on_full = clear)]
fn scaled(scale: f64, scratch: &mut Vec<f64>) -> f64 {
    scratch.push(scale);
    scale * 2.0
}

#[spanscope::memo(max_bytes = 1 << 20, value_size = value_bytes, verify = "VERIFY", verify_eq = same)]
fn words(text: &str) -> Vec<String> {
    text.split(' ').map(str::to_owned).collect()
}

fn value_bytes(value: &Vec<String>) -> usize {
    value.len() * 24
}

fn same(a: &Vec<String>, b: &Vec<String>) -> bool {
    a == b
}

#[derive(Clone, Default)]
struct Engine {
    cache: Cache<(u32, u32), Option<u64>>,
}

impl Engine {
    #[spanscope::memo(scope = field(self.cache), cache_none = true)]
    fn pair(&mut self, a: u32, b: u32) -> Option<u64> {
        a.checked_add(b).map(u64::from)
    }

    #[spanscope::memo(key = (self.cache.len(), value))]
    fn tagged<'a>(&self, value: &'a str) -> String {
        value.to_owned()
    }
}

fn main() {
    let mut scratch = Vec::new();
    assert_eq!(scaled(1.5, &mut scratch), 3.0);
    assert_eq!(words("a b").len(), 2);
    let mut engine = Engine::default();
    assert_eq!(engine.pair(1, 2), Some(3));
    assert_eq!(engine.tagged("x"), "x");
}
