// GTK4 settings app. Every change is saved instantly; the watermark picks it up live.
use gtk4::prelude::*;
use gtk4::{Adjustment, Application, ApplicationWindow, Entry, Grid, Label, Orientation, Scale, SpinButton};
use std::{cell::RefCell, rc::Rc};

#[path = "../config.rs"]
mod config;
use config::Config;

type Shared = Rc<RefCell<Config>>;

fn main() {
    // Start the watermark if it is not running yet.
    let running = std::process::Command::new("pgrep").args(["-f", "^(/usr/bin/)?activate-linux-watermark$"]).status().is_ok_and(|s| s.success());
    if !running {
        let _ = std::process::Command::new("activate-linux-watermark").spawn();
    }
    let app = Application::builder().application_id("io.github.theld21.ActivateLinuxWatermark").build();
    app.connect_activate(build);
    app.run();
}

/// Apply `f` to the shared config and save it.
fn update(cfg: &Shared, f: impl FnOnce(&mut Config)) {
    let mut c = cfg.borrow_mut();
    f(&mut c);
    c.save();
}

#[allow(deprecated)] // gtk-application-prefer-dark-theme: still the simplest way to force dark
fn build(app: &Application) {
    if let Some(s) = gtk4::Settings::default() {
        s.set_gtk_application_prefer_dark_theme(true);
    }
    let cfg: Shared = Rc::new(RefCell::new(Config::load()));
    let c = cfg.borrow().clone();
    let grid = Grid::builder().row_spacing(10).column_spacing(10)
        .margin_top(16).margin_bottom(16).margin_start(16).margin_end(16).build();
    let label = |text: &str, row| {
        let l = Label::builder().label(text).xalign(0.0).build();
        grid.attach(&l, 0, row, 1, 1);
    };

    // Text line + font size.
    let line = |row, title: &str, text: &str, size: f64, set_text: fn(&mut Config, String), set_size: fn(&mut Config, f64)| {
        label(title, row);
        let entry = Entry::builder().text(text).hexpand(true).width_chars(30).build();
        let spin = SpinButton::new(Some(&Adjustment::new(size, 6.0, 120.0, 1.0, 5.0, 0.0)), 1.0, 0);
        grid.attach(&entry, 1, row, 1, 1);
        grid.attach(&spin, 2, row, 1, 1);
        let c1 = cfg.clone();
        entry.connect_changed(move |e| update(&c1, |c| set_text(c, e.text().into())));
        let c2 = cfg.clone();
        spin.connect_value_changed(move |s| update(&c2, |c| set_size(c, s.value())));
    };
    line(0, "Line 1", &c.line1, c.size1, |c, v| c.line1 = v, |c, v| c.size1 = v);
    line(1, "Line 2", &c.line2, c.size2, |c, v| c.line2 = v, |c, v| c.size2 = v);

    // Margin and opacity sliders.
    let slider = |row, title: &str, max: f64, value: f64, set: fn(&mut Config, f64)| {
        label(title, row);
        let scale = Scale::with_range(Orientation::Horizontal, 0.0, max, 1.0);
        scale.set_value(value);
        scale.set_draw_value(true);
        scale.set_digits(0);
        grid.attach(&scale, 1, row, 2, 1);
        let c = cfg.clone();
        scale.connect_value_changed(move |s| update(&c, |c| set(c, s.value().round())));
    };
    slider(2, "Right margin", 1000.0, c.right, |c, v| c.right = v);
    slider(3, "Bottom margin", 1000.0, c.bottom, |c, v| c.bottom = v);
    slider(4, "Opacity (%)", 100.0, c.opacity, |c, v| c.opacity = v);

    ApplicationWindow::builder().application(app).title("Activate Linux Watermark").default_width(560)
        .child(&grid).build().present();
}
