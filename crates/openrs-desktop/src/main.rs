//! Native OpenRS desktop entry point.
//!
//! Startup opens the SQLite database named in [`startup::prepare_database_path`],
//! seeds the development catalog on the first run, and loads the catalog before
//! the first frame. A storage failure is shown in the window instead of being
//! logged and forgotten, because an empty catalog would otherwise look like a
//! store with no products.
//!
//! The repository stays open for the session so `Complete sale` can record a
//! sale directly. The shared UI only produces a checkout request; this shell
//! performs the call and hands the result back, which is the same division the
//! web shell uses with its storage worker.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod startup;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();

    let (app, repository) = open_startup_app();

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
        Box::new(|_creation_context| Ok(Box::new(DesktopApp { app, repository }))),
    )
}

/// Opens storage and loads the catalog, turning any failure into an error screen.
///
/// The desktop loads before the window exists, so it never shows the loading
/// state; the web shell does, because its database lives in a worker. The
/// repository is `None` only when startup failed, in which case there is nothing
/// to sell into.
#[cfg(not(target_arch = "wasm32"))]
fn open_startup_app() -> (
    openrs_ui::OpenRsApp,
    Option<openrs_storage::SqliteRepository>,
) {
    let opened = startup::prepare_database_path().and_then(|path| {
        let mut repository = startup::open_repository(&path)?;
        let catalog = startup::load_catalog(&mut repository)?;
        log::info!("database ready at {}", path.display());
        Ok((repository, catalog))
    });

    match opened {
        Ok((repository, catalog)) => (
            openrs_ui::OpenRsApp::with_catalog(catalog),
            Some(repository),
        ),
        Err(error) => {
            log::error!("catalog startup failed: {error}");
            (openrs_ui::OpenRsApp::with_error(error.to_string()), None)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct DesktopApp {
    app: openrs_ui::OpenRsApp,
    repository: Option<openrs_storage::SqliteRepository>,
}

#[cfg(not(target_arch = "wasm32"))]
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);

        // The click that asks for a checkout happens while `ui` draws, so the
        // request is taken afterwards; requesting one repaint keeps the result
        // from waiting for the next pointer event.
        if let Some(repository) = &mut self.repository
            && startup::complete_sale(repository, &mut self.app)
        {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // The native shell is not the web entry point. Use `openrs-web` with Trunk.
}
