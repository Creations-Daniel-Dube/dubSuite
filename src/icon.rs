use std::sync::Arc;

use eframe::egui::IconData;

pub fn app_icon() -> Arc<IconData> {
    let image = image::load_from_memory(include_bytes!("../images/dubSuite.png"))
        .expect("images/dubSuite.png")
        .into_rgba8();
    let (width, height) = image.dimensions();
    Arc::new(IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}
