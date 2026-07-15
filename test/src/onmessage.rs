//! Regression test for the `onmessage` lock in `src/webworker/js.rs`.
//!
//! The `#[wasm_bindgen(start)]` function below installs a conflicting
//! `onmessage` handler in every worker (start functions run during
//! `mod.default()`). The worker init code must clear and lock `onmessage`
//! after init so that task dispatch via `addEventListener` remains the
//! only message path.

use wasm_bindgen::{closure::Closure, prelude::wasm_bindgen, JsCast, JsValue};
use wasmworker::{webworker, webworker_fn, WebWorker, WebWorkerPool};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

use crate::{js_assert_eq, raw::sort};

/// Runs on every module initialization, both on the main page and inside
/// workers. Inside workers, install a hostile `onmessage` handler: if it
/// ever fires, it posts a message to the main thread that cannot be
/// deserialized as a task response, which fails the test run.
#[wasm_bindgen(start)]
fn register_conflicting_onmessage() {
    let Ok(scope) = js_sys::global().dyn_into::<DedicatedWorkerGlobalScope>() else {
        // Main page: `self` is a `Window`, nothing to do.
        return;
    };

    let post = scope.clone();
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |_: MessageEvent| {
        let _ = post.post_message(&JsValue::from_str("conflicting onmessage fired"));
    });
    scope.set_onmessage(Some(handler.as_ref().unchecked_ref()));
    handler.forget();
}

/// Runs inside a worker and reports whether its `onmessage` handler is cleared.
#[webworker_fn]
fn onmessage_is_cleared(_: Box<[u8]>) -> Box<[u8]> {
    let cleared = js_sys::global()
        .dyn_into::<DedicatedWorkerGlobalScope>()
        .map(|scope| scope.onmessage().is_none())
        .unwrap_or(false);
    vec![cleared as u8].into()
}

/// A conflicting `onmessage` registered by a `#[wasm_bindgen(start)]` function
/// must not break task dispatch. Covers both worker blobs: the regular one
/// (via [`WebWorker`]) and the precompiled-WASM one (via
/// [`WebWorkerPool::with_precompiled_wasm`]).
pub(crate) async fn can_run_task_with_conflicting_onmessage() {
    let vec: Box<[u8]> = vec![8, 1, 5, 0, 4].into();
    let sorted: Box<[u8]> = vec![0, 1, 4, 5, 8].into();
    let empty: Box<[u8]> = Vec::new().into();
    let cleared: Box<[u8]> = vec![1].into();

    // Regular worker blob.
    let worker = WebWorker::new(None).await.expect("Couldn't create worker");

    let res = worker.run_bytes(webworker!(sort), &vec).await;
    js_assert_eq!(
        res,
        sorted,
        "Task should complete despite conflicting onmessage"
    );

    let res = worker
        .run_bytes(webworker!(onmessage_is_cleared), &empty)
        .await;
    js_assert_eq!(res, cleared, "onmessage should be cleared in worker");

    // Precompiled WASM blob.
    let pool = WebWorkerPool::with_precompiled_wasm()
        .await
        .expect("Couldn't create pool with precompiled WASM");

    let res = pool.run_bytes(webworker!(sort), &vec).await;
    js_assert_eq!(
        res,
        sorted,
        "Task should complete despite conflicting onmessage (precompiled)"
    );

    let res = pool
        .run_bytes(webworker!(onmessage_is_cleared), &empty)
        .await;
    js_assert_eq!(
        res,
        cleared,
        "onmessage should be cleared in precompiled worker"
    );
}
