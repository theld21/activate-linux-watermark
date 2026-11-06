// "Activate Linux" watermark: one transparent, click-through, always-on-top
// X11 window per monitor (works on Wayland via XWayland).
use std::{thread, time::{Duration, Instant}};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::{xproto::*, Event};

mod config;
use config::Config;

fn load_font() -> fontdue::Font {
    let fc = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans"]).output();
    let fallback = ["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf"];
    let mut paths = fc.ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).into_iter().chain(fallback.map(String::from));
    let data = paths.find_map(|p| std::fs::read(p).ok()).expect("no font found");
    fontdue::Font::from_bytes(data, Default::default()).expect("invalid font")
}

/// Render both lines into a premultiplied BGRA buffer: (width, height, pixels).
fn render(font: &fontdue::Font, c: &Config) -> (u16, u16, Vec<u8>) {
    let lines = [(c.line1.as_str(), c.size1 as f32), (c.line2.as_str(), c.size2 as f32)];
    let metrics = |s| font.horizontal_line_metrics(s).unwrap();
    let text_w = |t: &str, s| t.chars().map(|ch| font.metrics(ch, s).advance_width).sum::<f32>();
    let w = lines.iter().map(|&(t, s)| text_w(t, s)).fold(0.0, f32::max).ceil() as usize + 4;
    let h = lines.iter().map(|&(_, s)| metrics(s).new_line_size).sum::<f32>().ceil() as usize + 4;
    let mut buf = vec![0u8; w * h * 4];
    let opacity = (c.opacity.clamp(0.0, 100.0) * 2.55) as u32;
    let mut y = 0.0;
    for (text, size) in lines {
        let lm = metrics(size);
        let base = (y + lm.ascent).round() as i32;
        let mut x = 2.0f32;
        for ch in text.chars() {
            let (m, bmp) = font.rasterize(ch, size);
            let (gx, gy) = (x.round() as i32 + m.xmin, base - m.height as i32 - m.ymin);
            for (i, &cov) in bmp.iter().enumerate() {
                let (px, py) = (gx + (i % m.width) as i32, gy + (i / m.width) as i32);
                if (0..w as i32).contains(&px) && (0..h as i32).contains(&py) {
                    let a = (cov as u32 * opacity / 255) as u8; // white, premultiplied
                    let o = (py as usize * w + px as usize) * 4;
                    buf[o..o + 4].fill(a);
                }
            }
            x += m.advance_width;
        }
        y += lm.new_line_size;
    }
    (w as u16, h as u16, buf)
}

/// Monitor rectangles (x, y, w, h); falls back to the whole screen.
fn monitors(conn: &impl Connection, root: Window, sw: u16, sh: u16) -> Vec<(i16, i16, u16, u16)> {
    let m: Vec<_> = conn.randr_get_monitors(root, true).ok().and_then(|c| c.reply().ok())
        .map(|r| r.monitors.iter().map(|m| (m.x, m.y, m.width, m.height)).collect()).unwrap_or_default();
    if m.is_empty() { vec![(0, 0, sw, sh)] } else { m }
}

/// Tiny xorshift PRNG seeded from the clock (no extra crate).
struct Rng(u64);
impl Rng {
    fn new() -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        Rng(t.as_nanos() as u64 | 1)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn sign(&mut self) -> f32 { if self.f() < 0.5 { -1.0 } else { 1.0 } }
}

const SPEED: f32 = 120.0; // bounce speed, px/s

/// DVD-logo style motion inside one monitor (position relative to the monitor).
struct Bounce { x: f32, y: f32, vx: f32, vy: f32 }

impl Bounce {
    fn random(rng: &mut Rng, (mw, mh): (f32, f32)) -> Self {
        let angle = (0.25 + rng.f() * 0.5) * std::f32::consts::FRAC_PI_2; // 22°..67°, never flat
        Bounce { x: rng.f() * mw, y: rng.f() * mh,
                 vx: rng.sign() * SPEED * angle.cos(), vy: rng.sign() * SPEED * angle.sin() }
    }
    /// Advance by `dt` seconds; reflect off edges (both at once = corner hit).
    fn step(&mut self, dt: f32, (max_x, max_y): (f32, f32)) {
        fn axis(p: &mut f32, v: &mut f32, max: f32) {
            if *p <= 0.0 { *p = -*p; *v = v.abs(); }
            if *p >= max { *p = 2.0 * max - *p; *v = -v.abs(); }
            *p = p.clamp(0.0, max);
        }
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        axis(&mut self.x, &mut self.vx, max_x);
        axis(&mut self.y, &mut self.vy, max_y);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let (root, sw, sh) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
    let visual = screen.allowed_depths.iter().filter(|d| d.depth == 32).flat_map(|d| &d.visuals)
        .find(|v| v.class == VisualClass::TRUE_COLOR).ok_or("no 32-bit visual")?.visual_id;
    let cmap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, cmap, root, visual)?;

    let font = load_font();
    let mtime = || std::fs::metadata(config::path()).and_then(|m| m.modified()).ok();
    let mut last = mtime();
    let mut cfg = Config::load();
    let (mut w, mut h, mut pixels) = render(&font, &cfg);
    let mut rng = Rng::new();
    let mut wins: Vec<(Window, Gcontext, Bounce)> = Vec::new();
    let mut mons = Vec::new();
    let mut prev = Instant::now();
    let (mut next_check, mut next_sync) = (prev, prev);
    let mut first = true;

    loop {
        let now = Instant::now();
        let dt = (now - prev).as_secs_f32().min(0.1);
        prev = now;
        let mut changed = std::mem::take(&mut first);
        // Every 0.5 s: reload if the config file changed (written by the settings app).
        if now >= next_check {
            next_check = now + Duration::from_millis(500);
            if mtime() != last {
                last = mtime();
                cfg = Config::load();
                (w, h, pixels) = render(&font, &cfg);
                changed = true;
            }
        }
        // Every 2 s: one window per monitor, resized and raised on top.
        if changed || now >= next_sync {
            next_sync = now + Duration::from_secs(2);
            mons = monitors(&conn, root, sw, sh);
            while wins.len() > mons.len() {
                let (win, gc, _) = wins.pop().unwrap();
                conn.free_gc(gc)?;
                conn.destroy_window(win)?;
            }
            while wins.len() < mons.len() {
                let win = conn.generate_id()?;
                let aux = CreateWindowAux::new().override_redirect(1).colormap(cmap).border_pixel(0)
                    .background_pixel(0).event_mask(EventMask::EXPOSURE);
                conn.create_window(32, win, root, 0, 0, w, h, 0, WindowClass::INPUT_OUTPUT, visual, &aux)?;
                // Empty input shape = clicks pass through.
                conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[])?;
                let gc = conn.generate_id()?;
                conn.create_gc(gc, win, &CreateGCAux::new())?;
                conn.map_window(win)?;
                let m = mons[wins.len()];
                let b = Bounce::random(&mut rng, ((m.2 - w.min(m.2)) as f32, (m.3 - h.min(m.3)) as f32));
                wins.push((win, gc, b));
            }
            for (&(win, gc, _), &(mx, my, mw, mh)) in wins.iter().zip(&mons) {
                let mut aux = ConfigureWindowAux::new().width(w as u32).height(h as u32).stack_mode(StackMode::ABOVE);
                if !cfg.bounce {
                    aux = aux.x(mx as i32 + mw as i32 - w as i32 - cfg.right as i32)
                        .y(my as i32 + mh as i32 - h as i32 - cfg.bottom as i32);
                }
                conn.configure_window(win, &aux)?;
                if changed {
                    conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w, h, 0, 0, 0, 32, &pixels)?;
                }
            }
        }
        if cfg.bounce {
            for ((win, _, b), &(mx, my, mw, mh)) in wins.iter_mut().zip(&mons) {
                b.step(dt, (mw.saturating_sub(w) as f32, mh.saturating_sub(h) as f32));
                let aux = ConfigureWindowAux::new().x(mx as i32 + b.x as i32).y(my as i32 + b.y as i32);
                conn.configure_window(*win, &aux)?;
            }
        }
        conn.flush()?;
        while let Some(ev) = conn.poll_for_event()? {
            if let Event::Expose(e) = ev {
                if let Some(&(win, gc, _)) = wins.iter().find(|x| x.0 == e.window) {
                    conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w, h, 0, 0, 0, 32, &pixels)?;
                }
            }
        }
        conn.flush()?;
        // ~60 fps while bouncing, otherwise idle.
        thread::sleep(Duration::from_millis(if cfg.bounce { 16 } else { 500 }));
    }
}
