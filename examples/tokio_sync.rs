#![cfg(feature = "thread-safe")]

use lazily::ThreadSafeContext;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let ctx = ThreadSafeContext::new();
    let input = ctx.cell(20usize);
    let doubled = ctx.computed(move |ctx| ctx.get(&input) * 2);

    let worker_ctx = ctx.clone();
    let result = tokio::task::spawn_blocking(move || {
        worker_ctx.set(&input, 21);
        worker_ctx.get(&doubled)
    })
    .await
    .expect("blocking task should finish");

    println!("doubled={result}");
}
