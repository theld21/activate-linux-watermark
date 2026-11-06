// GTK4 settings app. Every change is saved instantly; the watermark picks it up live.
use gtk4::prelude::*;
use gtk4::{glib, Adjustment, Align, Application, ApplicationWindow, Button, ButtonsType, CheckButton, Entry, Grid, Label, Orientation, MessageDialog, MessageType, ResponseType, Scale, SpinButton, Switch};
use std::{cell::RefCell, process::{Command, Stdio}, rc::Rc, time::Duration};

#[path = "../config.rs"]
mod config;
use config::Config;

type Shared = Rc<RefCell<Config>>;

fn main() {
    // Start the watermark if it is not running yet.
    let running = Command::new("pgrep").args(["-f", "^(/usr/bin/)?activate-linux-watermark$"]).status().is_ok_and(|s| s.success());
    if !running {
        let _ = Command::new("activate-linux-watermark").spawn();
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

    // On/off switch.
    let title = Label::builder().label("<b>Show watermark</b>").use_markup(true).xalign(0.0).build();
    let switch = Switch::builder().active(c.enabled).halign(Align::End).valign(Align::Center).build();
    grid.attach(&title, 0, 0, 2, 1);
    grid.attach(&switch, 2, 0, 1, 1);
    let cs = cfg.clone();
    switch.connect_active_notify(move |sw| update(&cs, |c| c.enabled = sw.is_active()));

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
    line(1, "Line 1", &c.line1, c.size1, |c, v| c.line1 = v, |c, v| c.size1 = v);
    line(2, "Line 2", &c.line2, c.size2, |c, v| c.line2 = v, |c, v| c.size2 = v);

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
    slider(3, "Right margin", 1000.0, c.right, |c, v| c.right = v);
    slider(4, "Bottom margin", 1000.0, c.bottom, |c, v| c.bottom = v);
    slider(5, "Opacity (%)", 100.0, c.opacity, |c, v| c.opacity = v);

    // DVD-logo style bouncing (ignores the margins).
    let bounce = CheckButton::builder().label("Bouncing watermark").active(c.bounce).build();
    grid.attach(&bounce, 1, 6, 2, 1);
    let cb = cfg.clone();
    bounce.connect_toggled(move |b| update(&cb, |c| c.bounce = b.is_active()));

    let uninstall = Button::builder().label("Uninstall…").halign(Align::End).margin_top(12).build();
    uninstall.add_css_class("destructive-action");
    grid.attach(&uninstall, 0, 7, 3, 1);

    let win = ApplicationWindow::builder().application(app).title("Activate Linux Watermark")
        .icon_name("io.github.theld21.ActivateLinuxWatermark").default_width(560).child(&grid).build();
    let w = win.clone();
    uninstall.connect_clicked(move |b| confirm_uninstall(&w, b));
    win.present();
}

const PKG: &str = "activate-linux-watermark";

/// Root command that removes the package, or None if it wasn't installed from a package.
fn remove_command() -> Option<[&'static str; 3]> {
    let out = |cmd: &str, args: &[&str]| Command::new(cmd).args(args).stderr(Stdio::null()).output()
        .ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    if out("dpkg-query", &["-W", "-f=${Status}", PKG]).is_some_and(|s| s.contains("ok installed")) {
        Some(["dpkg", "--purge", PKG])
    } else if out("rpm", &["-q", PKG]).is_some() {
        Some(["rpm", "-e", PKG])
    } else {
        None
    }
}

#[allow(deprecated)] // MessageDialog: AlertDialog needs GTK 4.10, Ubuntu 22.04 ships 4.6
fn dialog(win: &ApplicationWindow, kind: MessageType, text: &str, detail: &str) -> MessageDialog {
    let d = MessageDialog::builder().transient_for(win).modal(true).message_type(kind)
        .text(text).secondary_text(detail).buttons(ButtonsType::None).build();
    d
}

#[allow(deprecated)]
fn confirm_uninstall(win: &ApplicationWindow, button: &Button) {
    let Some(cmd) = remove_command() else {
        let d = dialog(win, MessageType::Info, "Not installed from a package",
            "Nothing to uninstall: this copy was not installed with the .deb or .rpm package.");
        d.add_button("OK", ResponseType::Ok);
        d.connect_response(|d, _| d.close());
        d.present();
        return;
    };
    let d = dialog(win, MessageType::Warning, "Uninstall Activate Linux Watermark?",
        "This removes the app, stops the watermark and deletes your settings.");
    d.add_button("Cancel", ResponseType::Cancel);
    d.add_button("Uninstall", ResponseType::Accept).add_css_class("destructive-action");
    let (win, button) = (win.clone(), button.clone());
    d.connect_response(move |d, r| {
        d.close();
        if r != ResponseType::Accept { return; }
        // pkexec shows the system password prompt; the package's prerm stops the watermark.
        let Ok(mut child) = Command::new("pkexec").args(cmd).spawn() else { return };
        button.set_sensitive(false);
        let (win, button) = (win.clone(), button.clone());
        glib::timeout_add_local(Duration::from_millis(200), move || match child.try_wait() {
            Ok(None) => glib::ControlFlow::Continue,
            Ok(Some(st)) if st.success() => {
                let _ = std::fs::remove_file(config::path());
                win.application().map(|a| a.quit());
                glib::ControlFlow::Break
            }
            _ => { // cancelled or failed: keep everything as is
                button.set_sensitive(true);
                glib::ControlFlow::Break
            }
        });
    });
    d.present();
}
