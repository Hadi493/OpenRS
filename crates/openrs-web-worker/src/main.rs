//! Dedicated browser worker that owns the OpenRS SQLite database.
//!
//! # Why a worker
//!
//! OPFS sync access handles only exist inside a dedicated worker, and the
//! `sahpool` VFS needs cross-origin isolation (ADR 0002). Reading the catalog on
//! the UI thread would therefore be impossible, not merely slow: SQLite must
//! live here, and the UI thread talks to it through `postMessage`.
//!
//! # Lifecycle
//!
//! 1. Open (or create) the OPFS database and apply the schema.
//! 2. Seed the development sample catalog on a fresh database, exactly once.
//! 3. Post the catalog, or `error\n<reason>` when startup failed. In the failure
//!    case no message handler is installed: the UI is already showing the
//!    startup error and there is nothing to check out into.
//! 4. Stay alive and answer checkout requests. The connection is kept open for
//!    the whole page, so a checkout is one transaction on an open database
//!    rather than a reopen per sale.
//!
//! # Protocol
//!
//! The decision logic is [`openrs_web_worker::handle_message`], which is shared
//! with the protocol tests and knows nothing about workers. This file only moves
//! strings: it decodes the browser's event, calls the handler, and posts the
//! returned text back to the UI. Message formats live in `openrs_core::wire`.
//!
//! There is no request correlation id beyond the checkout's own idempotency
//! key: only one checkout can be in flight per UI, and the key is echoed in the
//! reply so a late answer can be matched to its attempt.

#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use js_sys::Date;
    use openrs_core::{Catalog, catalog_message, error_message};
    use openrs_storage::{Clock, IdSource, RepoError, SqliteRepository};
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::prelude::Closure;
    use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

    /// OPFS file name of the database.
    const DATABASE: &str = "openrs.sqlite";

    /// Wall clock from the browser, as RFC 3339 in UTC.
    struct BrowserClock;

    impl Clock for BrowserClock {
        fn now_rfc3339(&self) -> String {
            Date::new_0().to_iso_string().into()
        }
    }

    /// Sale ids from the browser millisecond clock plus a counter.
    ///
    /// Enough until ADR 0002's UUIDv7 lands: the counter separates sales recorded
    /// in the same millisecond, and the timestamp separates page loads.
    struct SequenceIds(AtomicU64);

    impl IdSource for SequenceIds {
        fn next_sale_id(&self) -> String {
            let sequence = self.0.fetch_add(1, Ordering::Relaxed);
            format!("sale-{}-{sequence}", Date::now() as u64)
        }
    }

    /// Opens the OPFS database. Seeding is a separate step so a seeding failure
    /// is reported with its own message instead of looking like a broken file.
    async fn open_database() -> Result<SqliteRepository, RepoError> {
        SqliteRepository::open_opfs(
            DATABASE,
            Arc::new(BrowserClock),
            Arc::new(SequenceIds(AtomicU64::new(0))),
        )
        .await
    }

    /// Posts one string to the UI thread, logging instead of panicking when the
    /// port is already gone (the page navigated away).
    fn post(scope: &DedicatedWorkerGlobalScope, message: &str) {
        if let Err(error) = scope.post_message(&JsValue::from_str(message)) {
            web_sys::console::error_1(&JsValue::from_str(&format!(
                "OpenRS worker could not post a message: {error:?}"
            )));
        }
    }

    console_error_panic_hook::set_once();

    // Proves the worker binary actually started; the smoke test greps for the
    // "ready" line below, and this one tells a dead worker from a slow load.
    web_sys::console::info_1(&JsValue::from_str("OpenRS storage worker starting"));

    let scope = DedicatedWorkerGlobalScope::from(JsValue::from(js_sys::global()));

    wasm_bindgen_futures::spawn_local(async move {
        let mut repository = match open_database().await {
            Ok(repository) => repository,
            Err(error) => {
                web_sys::console::error_1(&JsValue::from_str(&format!(
                    "OpenRS database could not be opened: {error}"
                )));
                post(&scope, &error_message(&error.to_string()));
                return;
            }
        };

        let catalog =
            match openrs_storage::load_or_seed_catalog(&mut repository, &Catalog::sample()) {
                Ok((outcome, catalog)) => {
                    // Printed so the headless smoke test can prove the whole
                    // worker -> OPFS -> catalog path ran in a real browser.
                    web_sys::console::info_1(&JsValue::from_str(&format!(
                        "OpenRS catalog ready: {} products, {} variants ({outcome:?})",
                        catalog.products().len(),
                        catalog.variants().len()
                    )));
                    catalog
                }
                Err(error) => {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "OpenRS catalog startup failed: {error}"
                    )));
                    post(&scope, &error_message(&error.to_string()));
                    return;
                }
            };

        post(&scope, &catalog_message(&catalog));

        let reply_scope = scope.clone();
        let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(text) = event.data().as_string() else {
                post(
                    &reply_scope,
                    &openrs_core::checkout_failure_message(
                        "",
                        "the storage worker received a message it cannot read",
                    ),
                );
                return;
            };

            post(
                &reply_scope,
                &openrs_web_worker::handle_message(&mut repository, &text),
            );
        });

        scope.set_onmessage(Some(handler.as_ref().unchecked_ref()));

        // The worker lives as long as the page, so the closure is deliberately
        // leaked: freeing it would leave the browser calling released memory on
        // the next message.
        handler.forget();
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("openrs-web-worker is a WebAssembly target; it runs inside the browser.");
}
