// Shared config: $XDG_CONFIG_HOME/activate-linux-watermark.conf (key=value).
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config { pub line1: String, pub size1: f64, pub line2: String, pub size2: f64, pub right: f64, pub bottom: f64, pub opacity: f64, pub bounce: bool }

impl Default for Config {
    fn default() -> Self {
        Config { line1: "Activate Linux".into(), size1: 26.0, line2: "Go to Settings to activate Linux.".into(),
                 size2: 16.0, right: 90.0, bottom: 60.0, opacity: 40.0, bounce: false }
    }
}

pub fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("activate-linux-watermark.conf")
}

impl Config {
    pub fn load() -> Self {
        let mut c = Config::default();
        for l in std::fs::read_to_string(path()).unwrap_or_default().lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            let num = |d: f64| v.trim().parse().unwrap_or(d);
            match k.trim() {
                "line1" => c.line1 = v.into(),
                "line2" => c.line2 = v.into(),
                "size1" => c.size1 = num(c.size1),
                "size2" => c.size2 = num(c.size2),
                "right" => c.right = num(c.right),
                "bottom" => c.bottom = num(c.bottom),
                "opacity" => c.opacity = num(c.opacity),
                "bounce" => c.bounce = v.trim() == "1",
                _ => {}
            }
        }
        c
    }

    #[allow(dead_code)] // only used by the settings app
    pub fn save(&self) {
        let s = format!("line1={}\nsize1={}\nline2={}\nsize2={}\nright={}\nbottom={}\nopacity={}\nbounce={}\n",
            self.line1, self.size1, self.line2, self.size2, self.right, self.bottom, self.opacity, self.bounce as u8);
        let p = path();
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, s);
    }
}
