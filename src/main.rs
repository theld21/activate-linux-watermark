// "Activate Linux" watermark: one transparent, click-through, always-on-top
// X11 window per monitor (works on Wayland via XWayland).
use std::{thread, time::Duration};
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
    let mut wins: Vec<(Window, Gcontext)> = Vec::new();

    for tick in 0u64.. {
        // Reload when the config file changes (written by the settings app).
        let changed = mtime() != last;
        if changed {
            last = mtime();
            cfg = Config::load();
            (w, h, pixels) = render(&font, &cfg);
        }
        // Every 2 s: one window per monitor, bottom-right, raised on top.
        if changed || tick % 4 == 0 {
            let mons = monitors(&conn, root, sw, sh);
            while wins.len() > mons.len() {
                let (win, gc) = wins.pop().unwrap();
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
                wins.push((win, gc));
            }
            for (&(win, gc), &(mx, my, mw, mh)) in wins.iter().zip(&mons) {
                let x = mx as i32 + mw as i32 - w as i32 - cfg.right as i32;
                let y = my as i32 + mh as i32 - h as i32 - cfg.bottom as i32;
                let aux = ConfigureWindowAux::new().x(x).y(y).width(w as u32).height(h as u32);
                conn.configure_window(win, &aux.stack_mode(StackMode::ABOVE))?;
                if changed {
                    conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w, h, 0, 0, 0, 32, &pixels)?;
                }
            }
        }
        conn.flush()?;
        while let Some(ev) = conn.poll_for_event()? {
            if let Event::Expose(e) = ev {
                if let Some(&(win, gc)) = wins.iter().find(|x| x.0 == e.window) {
                    conn.put_image(ImageFormat::Z_PIXMAP, win, gc, w, h, 0, 0, 0, 32, &pixels)?;
                }
            }
        }
        conn.flush()?;
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}
