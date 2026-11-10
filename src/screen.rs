//! What the panel shows: boxes across the bar in the manner of a terminal monitor. Monospace
//! type, a thin frame per box with its name set into the top line, meters of small blocks and
//! dotted graphs that run from green through yellow to red.

use crate::canvas::{gradient, Canvas, Fonts, Rgb, H, HIDDEN_TOP, W};
use crate::stats::Stats;

const BG: Rgb = (8, 9, 11);
const FG: Rgb = (204, 204, 204);
const TITLE: Rgb = (240, 240, 240);
const DIM: Rgb = (118, 122, 128);
const EMPTY: Rgb = (44, 46, 50);
const GREEN: Rgb = (80, 210, 110);
const RED: Rgb = (240, 85, 75);

// one muted colour per box, for its frame
const BOX_CLOCK: Rgb = (72, 104, 124);
const BOX_CPU: Rgb = (85, 124, 92);
const BOX_MEM: Rgb = (124, 122, 78);
const BOX_GPU: Rgb = (104, 96, 160);
const BOX_AI: Rgb = (150, 110, 70);
const BOX_RUN: Rgb = (142, 86, 86);

const PAD: i32 = 20;

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if g >= 100.0 {
        format!("{g:.0}G")
    } else {
        format!("{g:.1}G")
    }
}

fn uptime(s: u64) -> String {
    let (d, h, m) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if d > 0 {
        format!("{d}d {h:02}h")
    } else {
        format!("{h}h {m:02}m")
    }
}

/// A temperature's place on the gradient: 35 °C and below is green, 90 °C red.
fn heat(celsius: f32) -> Rgb {
    gradient((celsius - 35.0) / 55.0)
}

pub struct Clock {
    pub time: String,
    pub date: String,
}

pub fn local_clock() -> Clock {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        libc::localtime_r(&now, &mut tm);
    }
    Clock {
        time: format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
        date: format!(
            "{} {:02} {}",
            DAYS[tm.tm_wday as usize % 7],
            tm.tm_mday,
            MONTHS[tm.tm_mon as usize % 12]
        ),
    }
}

struct Frame {
    x: i32,
    y: i32,
    w: i32,
}

impl Frame {
    fn left(&self) -> i32 {
        self.x + PAD
    }
    fn right(&self) -> i32 {
        self.x + self.w - PAD
    }
    fn inner(&self) -> i32 {
        self.w - 2 * PAD
    }
}

/// A box: a thin rounded line with `title` set into its top edge, and `note` at the right.
#[allow(clippy::too_many_arguments)]
fn frame(
    c: &mut Canvas,
    f: &mut Fonts,
    (x, y, w, h): (i32, i32, i32, i32),
    colour: Rgb,
    title: &str,
    note: Option<(&str, Rgb)>,
) -> Frame {
    c.rect(x, y, w, h, 10, colour);
    c.rect(x + 2, y + 2, w - 4, h - 4, 8, BG);
    let tw = f.width(title, 24, true);
    c.rect(x + 18, y, tw + 16, 2, 0, BG);
    c.text(f, x + 26, y + 9, 24, true, TITLE, title);
    if let Some((note, note_colour)) = note {
        let nw = f.width(note, 24, true);
        c.rect(x + w - 34 - nw, y, nw + 16, 2, 0, BG);
        c.text(f, x + w - 26 - nw, y + 9, 24, true, note_colour, note);
    }
    Frame { x, y, w }
}

/// "key        value" on one line.
fn row(c: &mut Canvas, f: &mut Fonts, k: &Frame, y: i32, key: &str, value: &str, colour: Rgb) {
    c.text(f, k.left(), y, 24, false, DIM, key);
    c.text_right(f, k.right(), y, 24, false, colour, value);
}

/// A share with its amounts: the name and "used/total", the percentage large, a meter.
fn usage(c: &mut Canvas, f: &mut Fonts, k: &Frame, y: i32, name: &str, used: u64, total: u64) {
    let pct = if total > 0 { used as f32 * 100.0 / total as f32 } else { 0.0 };
    row(c, f, k, y + 46, name, &format!("{}/{}", gb(used), gb(total)), FG);
    c.text(f, k.left(), y + 118, 68, true, gradient(pct / 100.0), &format!("{pct:.0}%"));
    c.meter(k.left(), y + 140, k.inner(), 18, pct / 100.0, EMPTY);
}

pub fn draw(f: &mut Fonts, s: &Stats, clock: &Clock, cpu_hist: &[f32], gpu_hist: &[f32]) -> Canvas {
    let mut c = Canvas::new(BG);
    let top = HIDDEN_TOP + 14;
    let height = H as i32 - top - 8;
    let (margin, gap) = (14, 12);
    let widths = [380, 380, 340, 380];
    let last = W as i32 - 2 * margin - 4 * gap - widths.iter().sum::<i32>();
    let xs: Vec<i32> = widths
        .iter()
        .scan(margin, |x, w| {
            let at = *x;
            *x += w + gap;
            Some(at)
        })
        .collect();
    let last_x = xs[3] + widths[3] + gap;

    // the time, the day, how long the machine has been up
    let host = if s.hostname.is_empty() { "host" } else { &s.hostname };
    let k = frame(&mut c, f, (xs[0], top, widths[0], height), BOX_CLOCK, host, None);
    let size = f.fit(&clock.time, 140, true, k.inner());
    let tw = f.width(&clock.time, size, true);
    c.text(f, k.x + (k.w - tw) / 2, k.y + 178, size, true, TITLE, &clock.time);
    let dw = f.width(&clock.date, 40, false);
    c.text(f, k.x + (k.w - dw) / 2, k.y + 244, 40, false, FG, &clock.date);
    row(&mut c, f, &k, k.y + 356, "up", &uptime(s.uptime_s), FG);
    row(&mut c, f, &k, k.y + 396, "load", &s.loadavg.replace("  ", " "), FG);

    // the processor
    let k = frame(&mut c, f, (xs[1], top, widths[1], height), BOX_CPU, "cpu", None);
    let load = s.cpu_load.unwrap_or(0.0);
    c.text(f, k.left(), k.y + 118, 96, true, gradient(load / 100.0), &format!("{load:.0}%"));
    if let Some(t) = s.cpu_temp {
        c.text_right(f, k.right(), k.y + 76, 36, true, heat(t), &format!("{t:.0}°C"));
    }
    c.meter(k.left(), k.y + 142, k.inner(), 18, load / 100.0, EMPTY);
    c.graph(k.left(), k.y + 186, k.inner(), 226, cpu_hist, EMPTY);

    // memory and the disk
    let k = frame(&mut c, f, (xs[2], top, widths[2], height), BOX_MEM, "mem", None);
    usage(&mut c, f, &k, k.y + 14, "ram", s.mem_used, s.mem_total);
    usage(&mut c, f, &k, k.y + 222, "disk /", s.disk_used, s.disk_total);

    // the graphics card
    let gpu_name = s.gpu.as_ref().map(|g| g.name.to_lowercase());
    let note = gpu_name.as_deref().map(|n| (n, DIM));
    let k = frame(&mut c, f, (xs[3], top, widths[3], height), BOX_GPU, "gpu", note);
    match &s.gpu {
        Some(g) => {
            let load = g.load as f32;
            c.text(f, k.left(), k.y + 118, 96, true, gradient(load / 100.0), &format!("{}%", g.load));
            c.text_right(f, k.right(), k.y + 76, 36, true, heat(g.temp as f32), &format!("{}°C", g.temp));
            c.text_right(f, k.right(), k.y + 118, 26, false, FG, &format!("{:.0}W", g.watts));
            c.meter(k.left(), k.y + 142, k.inner(), 18, load / 100.0, EMPTY);
            c.graph(k.left(), k.y + 186, k.inner(), 118, gpu_hist, EMPTY);
            let vram = g.mem_used as f32 / g.mem_total.max(1) as f32;
            row(&mut c, f, &k, k.y + 358, "vram", &format!("{}/{}", gb(g.mem_used), gb(g.mem_total)), FG);
            c.meter(k.left(), k.y + 376, k.inner(), 18, vram, EMPTY);
        }
        None => {
            c.text(f, k.left(), k.y + 76, 26, false, DIM, "not readable");
        }
    }

    // the model Ollama holds
    let ai_h = 132;
    let k = frame(&mut c, f, (last_x, top, last, ai_h), BOX_AI, "ollama", None);
    match &s.ollama {
        None => {
            c.text(f, k.left(), k.y + 76, 28, true, RED, "not answering");
        }
        Some(models) if models.is_empty() => {
            c.text(f, k.left(), k.y + 76, 28, false, DIM, "no model loaded");
        }
        Some(models) => {
            let (name, vram) = &models[0];
            let size = f.fit(name, 32, true, k.inner());
            c.text(f, k.left(), k.y + 62, size, true, TITLE, name);
            let more = if models.len() > 1 { format!(" +{}", models.len() - 1) } else { String::new() };
            c.text(f, k.left(), k.y + 102, 24, false, GREEN, &format!("{} in vram{more}", gb(*vram)));
        }
    }

    // the containers, stopped ones first
    let run_y = top + ai_h + gap + 6;
    let run = (last_x, run_y, last, height - ai_h - gap - 6);
    match &s.containers {
        None => {
            let k = frame(&mut c, f, run, BOX_RUN, "containers", None);
            c.text(f, k.left(), k.y + 62, 24, false, RED, "docker not readable");
        }
        Some(list) => {
            let running = list.iter().filter(|x| x.1).count();
            let count = format!("{running}/{}", list.len());
            let colour = if running == list.len() { GREEN } else { RED };
            let k = frame(&mut c, f, run, BOX_RUN, "containers", Some((&count, colour)));
            let rows = ((run.3 - 44) / 34) as usize;
            let shown = if list.len() > rows { rows - 1 } else { list.len() };
            for (i, (name, up)) in list.iter().take(shown).enumerate() {
                let y = k.y + 54 + i as i32 * 34;
                c.rect(k.left(), y - 15, 13, 13, 0, if *up { GREEN } else { RED });
                let size = f.fit(name, 24, false, k.inner() - 28 - 70);
                c.text(f, k.left() + 28, y, size, false, if *up { FG } else { RED }, name);
                c.text_right(f, k.right(), y, 24, false, if *up { DIM } else { RED }, if *up { "up" } else { "down" });
            }
            if list.len() > shown {
                let y = k.y + 54 + shown as i32 * 34;
                c.text(f, k.left() + 28, y, 24, false, DIM, &format!("+{} more", list.len() - shown));
            }
        }
    }
    c
}
