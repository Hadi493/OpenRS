//! Native OpenRS desktop entry point.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 700.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "OpenRS",
        native_options,
        Box::new(|_creation_context| Ok(Box::new(DesktopApp::new()))),
    )
}

#[cfg(not(target_arch = "wasm32"))]
struct DesktopApp {
    app: openrs_ui::OpenRsApp,
}

#[cfg(not(target_arch = "wasm32"))]
impl DesktopApp {
    fn new() -> Self {
        Self {
            app: openrs_ui::OpenRsApp::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // The native shell is not the web entry point. Use `openrs-web` with Trunk.
}
