//! turzx-stats: a stats screen for the TURZX 9.2" USB panel that sleeps between its pictures.
//!
//! One loop: read the numbers, draw one picture, send it if it differs from the last one,
//! sleep until the next update. No threads, no timers in between.

mod canvas;
mod proto;
mod screen;
mod stats;

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

struct Options {
    interval: u64,
    brightness: u8,
    fonts: String,
    png: Option<String>,
    once: bool,
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        interval: 5,
        brightness: 60,
        fonts: "/usr/share/fonts/truetype/dejavu".into(),
        png: None,
        once: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--interval" => {
                o.interval = value("--interval")?.parse().map_err(|_| "--interval: seconds")?;
                if o.interval == 0 {
                    return Err("--interval: at least 1".into());
                }
            }
            "--brightness" => {
                o.brightness = value("--brightness")?.parse().map_err(|_| "--brightness: 0-100")?
            }
            "--fonts" => o.fonts = value("--fonts")?,
            "--png" => o.png = Some(value("--png")?),
            "--once" => o.once = true,
            "-h" | "--help" => {
                println!(
                    "turzx-stats [--interval SECONDS] [--brightness 0-100] [--fonts DIR]\n\
                     \x20           [--once] [--png FILE]\n\n\
                     --interval    seconds between updates (default 5)\n\
                     --brightness  panel brightness in percent (default 60)\n\
                     --fonts       folder with DejaVuSans.ttf and DejaVuSans-Bold.ttf\n\
                     --once        send one picture and leave it on the panel\n\
                     --png FILE    write the picture to FILE instead of the panel"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other} (see --help)")),
        }
    }
    Ok(o)
}

/// Sleep until the wall clock's next multiple of `interval`, so the minute turns on time.
/// One sleep, which a signal ends early.
fn sleep_to_next(interval: u64) {
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) };
    let into = now.tv_sec as u64 % interval;
    let nanos = (interval - into) * 1_000_000_000 - now.tv_nsec as u64;
    let wait = libc::timespec {
        tv_sec: (nanos / 1_000_000_000) as libc::time_t,
        tv_nsec: (nanos % 1_000_000_000) as libc::c_long,
    };
    unsafe { libc::nanosleep(&wait, std::ptr::null_mut()) };
}

fn main() {
    let o = match options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("turzx-stats: {e}");
            std::process::exit(2);
        }
    };
    let mut fonts = match canvas::Fonts::load(&o.fonts) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("turzx-stats: {e}");
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }

    let mut reader = stats::Reader::new();
    let mut cpu_hist: Vec<f32> = Vec::new();
    let mut gpu_hist: Vec<f32> = Vec::new();
    let mut panel: Option<proto::Panel> = None;
    let mut last_sent: Vec<u8> = Vec::new();
    let mut complained = false;

    if o.png.is_some() || o.once {
        // the load is a difference between two readings: give it something to measure
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    loop {
        let s = reader.read();
        for (hist, value) in [
            (&mut cpu_hist, s.cpu_load.unwrap_or(0.0)),
            (&mut gpu_hist, s.gpu.as_ref().map(|g| g.load as f32).unwrap_or(0.0)),
        ] {
            hist.push(value);
            if hist.len() > 120 {
                hist.remove(0);
            }
        }
        let picture = screen::draw(&mut fonts, &s, &screen::local_clock(), &cpu_hist, &gpu_hist);

        if let Some(path) = &o.png {
            if let Err(e) = std::fs::write(path, picture.to_landscape_png()) {
                eprintln!("turzx-stats: {path}: {e}");
                std::process::exit(1);
            }
            return;
        }

        if picture.px != last_sent {
            if panel.is_none() {
                match proto::Panel::open().and_then(|p| p.set_brightness(o.brightness).map(|_| p)) {
                    Ok(p) => {
                        eprintln!("turzx-stats: panel connected");
                        complained = false;
                        panel = Some(p);
                    }
                    Err(e) => {
                        if !complained {
                            eprintln!("turzx-stats: {e}; trying again every {} s", o.interval);
                            complained = true;
                        }
                    }
                }
            }
            if let Some(p) = &panel {
                match p.send_png(&picture.to_png()) {
                    Ok(()) => last_sent = picture.px,
                    Err(e) => {
                        // a replug or a sleeping port: a fresh connection at the next update
                        eprintln!("turzx-stats: {e}; reconnecting");
                        panel = None;
                        last_sent.clear();
                    }
                }
            }
        }

        if o.once || STOP.load(Ordering::SeqCst) {
            break;
        }
        sleep_to_next(o.interval);
        if STOP.load(Ordering::SeqCst) {
            break;
        }
    }

    // stopped on purpose: a dark panel says so, stale numbers would not
    if !o.once {
        if let Some(p) = &panel {
            let _ = p.set_brightness(0);
        }
    }
}
