//! work.rs

#[test]
fn threadpool_test_work() {
    let pool = msft_runtime::work::once(|_| 42).unwrap();
    let result = futures::executor::block_on(pool.future());
    assert_eq!(42, result)
}
