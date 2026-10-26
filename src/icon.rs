use std::sync::Arc;

use eframe::egui::IconData;

pub fn app_icon() -> Arc<IconData> {
    const N: u32 = 64;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let dx = x as i32 - 32;
            let dy = y as i32 - 32;
            if dx * dx + dy * dy > 30 * 30 {
                continue;
            }
            let index = ((y * N + x) * 4) as usize;
            let bar = (18..46).contains(&x)
                && ((18..24).contains(&y) || (30..36).contains(&y) || (42..48).contains(&y));
            if bar {
                rgba[index] = 0xe6;
                rgba[index + 1] = 0xa8;
                rgba[index + 2] = 0x17;
            } else {
                rgba[index] = 0x1c;
                rgba[index + 1] = 0x1c;
                rgba[index + 2] = 0x20;
            }
            rgba[index + 3] = 255;
        }
    }
    Arc::new(IconData {
        rgba,
        width: N,
        height: N,
    })
}
