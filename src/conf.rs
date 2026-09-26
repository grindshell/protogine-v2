//! Game configuration, filled in by `pg.conf(t)` in `conf.lua`.

use macroquad::miniquad::conf::{Conf as WindowConf, Platform};

#[derive(Clone, Debug)]
pub struct Conf {
    pub window: Window,
    /// Upper bound on `dt`, in seconds.
    pub maxdelta: f64,
}

#[derive(Clone, Debug)]
pub struct Window {
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub resizable: bool,
    pub fullscreen: bool,
    pub highdpi: bool,
    pub msaa: i32,
    pub vsync: bool,
}

impl Default for Conf {
    fn default() -> Self {
        Conf {
            window: Window {
                title: "protogine".to_string(),
                width: 800,
                height: 600,
                resizable: false,
                fullscreen: false,
                highdpi: false,
                msaa: 0,
                vsync: true,
            },
            maxdelta: 10.0,
        }
    }
}

impl Conf {
    pub fn window_conf(&self) -> WindowConf {
        let w = &self.window;
        WindowConf {
            window_title: w.title.clone(),
            window_width: w.width,
            window_height: w.height,
            window_resizable: w.resizable,
            fullscreen: w.fullscreen,
            high_dpi: w.highdpi,
            sample_count: w.msaa.max(1),
            platform: Platform {
                swap_interval: Some(i32::from(w.vsync)),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
