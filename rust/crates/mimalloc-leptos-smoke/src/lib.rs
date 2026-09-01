//! Leptos `reactive_graph` churn through [`mimalloc_core::Mimalloc`].
//!
//! This is the in-tree WASM stress: thousands of signals/memos, nested owners,
//! string/vec realloc. The harness also clones `leptos-rs/leptos` and runs its
//! own `cargo test` crates on `wasm32-wasip1`.

#![no_std]

extern crate alloc;

use mimalloc_core::Mimalloc;

#[global_allocator]
static ALLOC: Mimalloc = Mimalloc;

use alloc::string::ToString;
use alloc::vec::Vec;
use any_spawner::Executor;
use reactive_graph::computed::ArcMemo;
use reactive_graph::owner::Owner;
use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::{Get, Set, Update};

/// Returns 0 on success, a probe id on failure.
pub fn run() -> i32 {
    let _ = Executor::init_futures_executor();
    let owner = Owner::new();
    owner.with(|| work())
}

fn work() -> i32 {
    const N: usize = 512;
    let mut sigs = Vec::with_capacity(N);
    for i in 0..N {
        sigs.push(ArcRwSignal::new(i as i32));
    }
    let first = sigs[0].clone();
    let last = sigs[N - 1].clone();
    let memo = ArcMemo::new(move |_| first.get() + last.get());
    if memo.get() != (N as i32 - 1) {
        return 1;
    }
    sigs[0].set(10);
    sigs[N - 1].update(|v| *v += 1);
    if memo.get() != 10 + N as i32 {
        return 2;
    }

    let mut names = Vec::new();
    for i in 0..256 {
        names.push(i.to_string().repeat(8));
    }
    names.truncate(64);
    names.shrink_to_fit();
    if names[0] != "00000000" || names.len() != 64 {
        return 3;
    }

    let nested = Owner::new();
    let nested_ok = nested.with(|| {
        let s = ArcRwSignal::new(Vec::from([1u8; 128]));
        s.update(|v| v.extend_from_slice(&[2u8; 128]));
        s.get().len() == 256
    });
    if !nested_ok {
        return 4;
    }
    drop(nested);

    for s in &sigs {
        s.set(0);
    }
    drop(sigs);
    drop(memo);
    drop(names);
    0
}
