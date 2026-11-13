//! Timing for SILICA_BENCH runs: how long parts of a frame take.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Instant;

thread_local! {
    static TIMES: RefCell<BTreeMap<&'static str, (f32, f32, usize)>> = RefCell::new(BTreeMap::new());
}

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SILICA_BENCH").is_some())
}

pub fn time<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    let ms = start.elapsed().as_secs_f32() * 1000.;
    TIMES.with(|t| {
        let mut t = t.borrow_mut();
        let entry = t.entry(name).or_insert((0., 0., 0));
        entry.0 += ms;
        entry.1 = entry.1.max(ms);
        entry.2 += 1;
    });
    result
}

pub fn report() {
    TIMES.with(|t| {
        let mut t = t.borrow_mut();
        for (name, (total, max, count)) in t.iter() {
            eprintln!("   {name:<24} avg {:.2}ms max {:.2}ms", total / *count as f32, max);
        }
        t.clear();
    });
}
