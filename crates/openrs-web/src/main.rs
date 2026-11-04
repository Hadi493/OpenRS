//! WebAssembly OpenRS web entry point.
//!
//! Trunk compiles this crate to `wasm32-unknown-unknown` and injects the
//! generated wasm-bindgen loader. The application startup is Rust code; the
//! HTML file contains no handwritten JavaScript application code.
//!
//! # Storage worker
//!
//! The browser database lives in [`openrs_web_worker`], a dedicated worker, so
//! the UI thread cannot read SQLite itself. This shell starts that worker and
//! talks to it with the [`openrs_core`] worker protocol: the worker posts a
//! startup message, and the web shell can later send checkout requests and
//! receive their replies. Until the startup message arrives the screen shows
//! the loading state; a failure becomes the error state with the worker's
//! reason, never an empty catalog.
//!
//! The worker's script is the loader shim Trunk generates for the worker
//! binary. Its file name is stable because `Trunk.toml` disables file hashing,
//! which is what lets Rust start the worker by URL without hand-written
//! JavaScript.

#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();
    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    let web_options = eframe::WebOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    wasm_bindgen_futures::spawn_local(async move {
        let document = web_sys::window()
            .expect("the browser window should exist")
            .document()
            .expect("the browser document should exist");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("index.html should contain a canvas with id `the_canvas_id`")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("`the_canvas_id` should be an HTML canvas element");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|creation_context| Ok(Box::new(WebApp::new(&creation_context.egui_ctx)))),
            )
            .await;

        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(()) => loading_text.remove(),
                Err(error) => {
                    loading_text.set_inner_html(
                        "<p>OpenRS could not start. See the browser console for details.</p>",
                    );
                    panic!("failed to start OpenRS in the browser: {error:?}");
                }
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
struct WebApp {
    app: openrs_ui::OpenRsApp,
    storage: StorageWorker,
}

#[cfg(target_arch = "wasm32")]
impl WebApp {
    fn new(context: &eframe::egui::Context) -> Self {
        Self {
            // The worker starts in the loading state, so the first frame shows
            // that startup is in progress rather than an empty catalog.
            app: openrs_ui::OpenRsApp::new(),
            storage: StorageWorker::start(context),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        let mut busy = false;

        for message in self.storage.take_replies() {
            busy = true;
            match message {
                openrs_core::WorkerMessage::Catalog(openrs_core::CatalogResult { ok, err }) => {
                    if let Some(catalog) = ok {
                        self.app.set_catalog(catalog);
                    } else if let Some(reason) = err {
                        self.app.set_error(reason);
                    }
                }
                openrs_core::WorkerMessage::Checkout(reply) => match reply {
                    openrs_core::CheckoutReply::Recorded { receipt, catalog } => {
                        self.app.set_catalog(catalog);
                        self.app.checkout_recorded(receipt);
                    }
                    openrs_core::CheckoutReply::Failed { checkout_id, reason } => {
                        if !self.app.checkout_failed(&checkout_id, &reason) {
                            log::warn!(
                                "checkout reply for unknown attempt `{checkout_id}`: {reason}"
                            );
                            self.app.checkout_aborted(reason);
                        }
                    }
                },
            }
        }

        self.app.ui(ui);

        if let Some(request) = self.app.take_checkout_request() {
            if let Err(reason) = self.storage.send_checkout(&request) {
                log::error!(
                    "could not send checkout {checkout_id}: {reason}",
                    checkout_id = request.checkout_id
                );
                self.app.checkout_failed(&request.checkout_id, reason);
            }
            busy = true;
        }

        if busy {
            ui.ctx().request_repaint();
        }
    }
}

/// The worker script Trunk generates for the worker binary.
///
/// Kept in one place so a rename of the binary or the build settings shows up
/// here. `Trunk.toml` sets `filehash = false`, which is what makes this path
/// stable.
#[cfg(target_arch = "wasm32")]
const WORKER_SCRIPT: &str = "./openrs-web-worker_loader.js";

/// UI-thread side of the storage worker handshake.
///
/// The worker posts exactly one startup message. It is stored in a shared slot
/// instead of being applied directly because the callback cannot borrow the
/// application state: the message arrives on the JavaScript event loop while
/// eframe owns `WebApp`. The next frame picks it up in [`WebApp::ui`].
#[cfg(target_arch = "wasm32")]
struct StorageWorker {
    slot: std::sync::Arc<std::sync::Mutex<Vec<openrs_core::WorkerMessage>>>,
    /// Kept alive on purpose: dropping the handle terminates the worker, and
    /// dropping the closure would leave the JavaScript handler pointing at freed
    /// memory.
    worker: Option<web_sys::Worker>,
    /// See [`Self::worker`].
    on_message: Option<eframe::wasm_bindgen::prelude::Closure<dyn FnMut(web_sys::MessageEvent)>>,
}

#[cfg(target_arch = "wasm32")]
impl CatalogWorker {
    /// Starts the worker and registers the message handler.
    ///
    /// A worker that cannot be created is reported through the same slot as a
    /// failed load, so the screen has one failure path.
    ///
    /// The struct itself is unnamed, but keep the field names stable so a later
    /// refactor is mechanical: `StorageWorker::start`, `StorageWorker::send`,
    /// and `StorageWorker::replies`.
    fn start(context: &eframe::egui::Context) -> Self {
        use eframe::wasm_bindgen::JsCast as _;
        use eframe::wasm_bindgen::prelude::Closure;
        use std::sync::{Arc, Mutex};

        let slot = Arc::new(Mutex::new(Vec::new()));

        let handler = {
            let slot = Arc::clone(&slot);
            let context = context.clone();
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                let message = match event.data().as_string() {
                    Some(text) => openrs_core::decode_worker_message(&text),
                    None => openrs_core::WorkerMessage::Catalog(Err(String::from(
                        "the storage worker sent a message this build cannot read",
                    ))),
                };

                lock(&slot).push(message);
                // Without this the app would stay on the loading frame: egui only
                // repaints when something asks it to.
                context.request_repaint();
            })
        };

        match web_sys::Worker::new(WORKER_SCRIPT) {
            Ok(worker) => {
                worker.set_onmessage(Some(handler.as_ref().unchecked_ref()));
                log::info!("storage worker started");
                Self {
                    slot,
                    worker: Some(worker),
                    on_message: Some(handler),
                }
            }
            Err(error) => {
                lock(&slot).push(openrs_core::WorkerMessage::Catalog(Err(format!(
                    "the storage worker could not be started: {error:?}"
                ))));
                Self {
                    slot,
                    worker: None,
                    on_message: None,
                }
            }
        }
    }

    /// Sends a checkout request to the worker; the reply arrives through the
    /// message handler.
    fn send_checkout(
        &self,
        request: &openrs_core::CheckoutRequest,
    ) -> Result<(), String> {
        let Ok(worker) = &self.worker else {
            return Err(String::from(
                "the storage worker is not running, so the sale cannot be sent",
            ));
        };

        worker
            .post_message(&eframe::wasm_bindgen::JsValue::from_str(
                &openrs_core::checkout_request_message(request),
            ))
            .map_err(|error| format!(
                "the checkout could not be sent to the storage worker: {error:?}"
            ))
    }

    /// Takes all worker replies since the last call.
    fn take_replies(&mut self) -> Vec<openrs_core::WorkerMessage> {
        lock(&self.slot).drain(..).collect()
    }
}

/// Locks a shared slot.
///
/// Both the startup and reply slots live behind a shared `Mutex`, so the lock
/// helper is reused. A poisoned lock in the web target means a panic happened
/// while the slot was held; recovering the value is still the most useful
/// behavior.
#[cfg(target_arch = "wasm32")]
fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("openrs-web is a WebAssembly target. Build it with Trunk.");
}
