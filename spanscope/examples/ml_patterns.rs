//! Produces runs with two shapes, a regression, slow outliers, quadratic growth,
//! and one tracked object that changes far more than its peers, so every
//! pattern section of the `ml` analysis has something to find.
//!
//! Run with `cargo run -p spanscope --features ml,object-tracking --example ml_patterns`.

use spanscope::{metric, trace, InstanceId, Tracked};
use std::hint::black_box;

fn spin(iterations: u64) {
    let mut value = 0u64;
    for step in 0..iterations {
        value = black_box(value.wrapping_add(step));
    }
    black_box(value);
}

#[derive(Tracked)]
struct Session {
    #[tracked(id)]
    id: InstanceId,
    requests: u64,
    last_size: u64,
}

#[trace]
fn lookup_cache(size: u64) {
    spin(2_000 + size * 20);
}

#[trace]
fn query_database(size: u64) {
    spin(150_000 + size * 300);
}

/// Deliberately quadratic.
#[trace]
fn sort_items(size: u64) {
    let mut items = (0..size).rev().collect::<Vec<_>>();
    for i in 0..items.len() {
        for j in 0..items.len() - 1 - i {
            if items[j] > items[j + 1] {
                items.swap(j, j + 1);
            }
        }
    }
    black_box(items);
}

#[trace]
fn audit_log() {
    spin(250_000);
}

#[trace]
fn slow_path() {
    spin(3_000_000);
}

#[trace(track(session))]
fn touch_session(session: &mut Session, size: u64) {
    session.requests += 1;
    session.last_size = size;
}

#[trace(root)]
fn handle_request(request: u64, session: &mut Session) {
    let size = 20 + (request * 37) % 180;
    metric!("input_size", size);
    if request % 3 == 0 {
        query_database(size);
    } else {
        lookup_cache(size);
    }
    sort_items(size);
    if request >= 50 {
        audit_log();
    }
    if request == 17 || request == 61 {
        slow_path();
    }
    touch_session(session, size);
}

fn main() {
    spanscope::config::ConfigBuilder::new()
        .program("ml_patterns")
        .output("target/spanscope-ml-patterns.json")
        .init()
        .expect("spanscope configuration");
    let mut sessions = (0..12)
        .map(|_| Session {
            id: InstanceId::new(),
            requests: 0,
            last_size: 0,
        })
        .collect::<Vec<_>>();
    for request in 0..80 {
        // Session 0 is a hot key: it receives every other request.
        let user = if request % 2 == 0 {
            0
        } else {
            (request as usize % 11) + 1
        };
        handle_request(request, &mut sessions[user]);
    }
    let path = spanscope::export::flush().expect("profile export");
    println!("profile written to {}", path.display());
}
