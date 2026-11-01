//! WebAssembly OpenRS web entry point.
//!
//! Trunk compiles this crate to `wasm32-unknown-unknown` and injects the
//! generated wasm-bindgen loader. The application startup is Rust code; the
//! HTML file contains no handwritten JavaScript application code.

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
                Box::new(|_creation_context| Ok(Box::new(WebApp::new()))),
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
}

#[cfg(target_arch = "wasm32")]
impl WebApp {
    fn new() -> Self {
        Self {
            app: openrs_ui::OpenRsApp::new(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("openrs-web is a WebAssembly target. Build it with Trunk.");
}
