mod app;
mod backend;
mod ui;

use app::EmuApp;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    use eframe::*;

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_minimize_button(true)
            .with_maximize_button(true)
            .with_close_button(true)
            .with_maximized(true),
        ..Default::default()
    };
    run_native(
        "HyperEmu Emulator",
        options,
        Box::new(|cc| Ok(Box::new(EmuApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::*;
    use wasm_bindgen::JsCast;

    wasm_bindgen_futures::spawn_local(async {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");

        let canvas = document
            .get_element_by_id("emulator_canvas")
            .expect("canvas not found")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("not a canvas");

        let web_options = WebOptions::default();

        WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(EmuApp::new(cc)))),
            )
            .await
            .expect("failed to start");
    });
}
