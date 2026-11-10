//! What the panel shows: five cards across the bar.

use crate::canvas::{Canvas, Fonts, Rgb, H, HIDDEN_TOP, W};
use crate::stats::Stats;

const BG: Rgb = (9, 11, 16);
const CARD: Rgb = (21, 25, 34);
const TRACK: Rgb = (42, 48, 62);
const TEXT: Rgb = (232, 235, 241);
const DIM: Rgb = (139, 147, 163);
const ACCENT: Rgb = (61, 219, 217);
const GREEN: Rgb = (74, 222, 128);
const AMBER: Rgb = (251, 191, 36);
const RED: Rgb = (248, 113, 113);

const PAD: i32 = 22;

fn level(value: f32, amber: f32, red: f32) -> Rgb {
    if value >= red {
        RED
    } else if value >= amber {
        AMBER
    } else {
        ACCENT
    }
}

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if g >= 100.0 {
        format!("{g:.0}")
    } else {
        format!("{g:.1}")
    }
}

fn uptime(s: u64) -> String {
    let (d, h, m) = (s / 86400, s % 86400 / 3600, s % 3600 / 60);
    if d > 0 {
        format!("up {d}d {h}h")
    } else if h > 0 {
        format!("up {h}h {m}m")
    } else {
        format!("up {m}m")
    }
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
            "{} {} {}",
            DAYS[tm.tm_wday as usize % 7],
            tm.tm_mday,
            MONTHS[tm.tm_mon as usize % 12]
        ),
    }
}

struct Card {
    x: i32,
    y: i32,
    w: i32,
}

impl Card {
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

pub fn draw(f: &mut Fonts, s: &Stats, clock: &Clock, cpu_hist: &[f32], gpu_hist: &[f32]) -> Canvas {
    let mut c = Canvas::new(BG);
    let top = HIDDEN_TOP + 10;
    let height = H as i32 - top - 10;
    let gap = 14;
    let widths = [400, 340, 340, 360, 0];
    let last = W as i32 - 2 * 20 - 4 * gap - widths.iter().sum::<i32>();
    let mut x = 20;
    let mut cards = Vec::new();
    for w in widths {
        let w = if w == 0 { last } else { w };
        c.rect(x, top, w, height, 18, CARD);
        cards.push(Card { x, y: top, w });
        x += w + gap;
    }

    // 1: the time, the day, the machine
    let k = &cards[0];
    let size = f.fit(&clock.time, 150, true, k.inner());
    let tw = f.width(&clock.time, size, true);
    c.text(f, k.x + (k.w - tw) / 2, k.y + 190, size, true, TEXT, &clock.time);
    let dw = f.width(&clock.date, 46, false);
    c.text(f, k.x + (k.w - dw) / 2, k.y + 262, 46, false, ACCENT, &clock.date);
    let host = format!("{}  ·  {}", s.hostname, uptime(s.uptime_s));
    let hs = f.fit(&host, 30, false, k.inner());
    let hw = f.width(&host, hs, false);
    c.text(f, k.x + (k.w - hw) / 2, k.y + 390, hs, false, DIM, &host);

    // 2: the processor
    let k = &cards[1];
    c.text(f, k.left(), k.y + 52, 30, true, DIM, "CPU");
    if let Some(t) = s.cpu_temp {
        c.text_right(f, k.right(), k.y + 52, 30, false, level(t, 70.0, 85.0), &format!("{t:.0}°C"));
    }
    let load = s.cpu_load.unwrap_or(0.0);
    c.text(f, k.left(), k.y + 170, 110, true, TEXT, &format!("{load:.0}%"));
    c.bar(k.left(), k.y + 196, k.inner(), 16, load / 100.0, TRACK, level(load, 75.0, 90.0));
    c.history(k.left(), k.y + 236, k.inner(), 120, cpu_hist, TRACK, ACCENT);
    c.text(f, k.left(), k.y + 404, 26, false, DIM, &format!("load  {}", s.loadavg));

    // 3: memory and the disk
    let k = &cards[2];
    let mut usage = |c: &mut Canvas, y: i32, title: &str, used: u64, total: u64| {
        let pct = if total > 0 { used as f32 * 100.0 / total as f32 } else { 0.0 };
        c.text(f, k.left(), y + 52, 30, true, DIM, title);
        c.text(f, k.left(), y + 136, 76, true, TEXT, &format!("{pct:.0}%"));
        // under the bar: beside the big number the amounts ran into it
        c.text(f, k.left(), y + 202, 24, false, DIM, &format!("{} / {} GB", gb(used), gb(total)));
        c.bar(k.left(), y + 160, k.inner(), 16, pct / 100.0, TRACK, level(pct, 75.0, 90.0));
    };
    usage(&mut c, k.y, "MEMORY", s.mem_used, s.mem_total);
    usage(&mut c, k.y + 212, "DISK  /", s.disk_used, s.disk_total);

    // 4: the graphics card
    let k = &cards[3];
    c.text(f, k.left(), k.y + 52, 30, true, DIM, "GPU");
    match &s.gpu {
        Some(g) => {
            let t = g.temp as f32;
            c.text_right(f, k.right(), k.y + 52, 30, false, level(t, 75.0, 85.0), &format!("{t:.0}°C"));
            c.text(f, k.left(), k.y + 170, 110, true, TEXT, &format!("{}%", g.load));
            c.text_right(f, k.right(), k.y + 170, 34, false, DIM, &format!("{:.0} W", g.watts));
            c.bar(k.left(), k.y + 196, k.inner(), 16, g.load as f32 / 100.0, TRACK, level(g.load as f32, 101.0, 101.0));
            c.history(k.left(), k.y + 236, k.inner(), 60, gpu_hist, TRACK, ACCENT);
            let vram = g.mem_used as f32 / g.mem_total.max(1) as f32;
            c.text(f, k.left(), k.y + 350, 28, true, DIM, "VRAM");
            c.text_right(f, k.right(), k.y + 350, 28, false, TEXT, &format!("{} / {} GB", gb(g.mem_used), gb(g.mem_total)));
            c.bar(k.left(), k.y + 368, k.inner(), 16, vram, TRACK, level(vram * 100.0, 85.0, 97.0));
            let ns = f.fit(&g.name, 24, false, k.inner());
            c.text(f, k.left(), k.y + 420, ns, false, DIM, &g.name);
        }
        None => {
            c.text(f, k.left(), k.y + 170, 40, false, DIM, "not readable");
        }
    }

    // 5: what runs
    let k = &cards[4];
    c.text(f, k.left(), k.y + 52, 30, true, DIM, "OLLAMA");
    match &s.ollama {
        None => {
            c.text(f, k.left(), k.y + 104, 34, true, RED, "not answering");
        }
        Some(models) if models.is_empty() => {
            c.text(f, k.left(), k.y + 104, 34, false, DIM, "no model loaded");
        }
        Some(models) => {
            let (name, vram) = &models[0];
            let ns = f.fit(name, 40, true, k.inner());
            c.text(f, k.left(), k.y + 104, ns, true, TEXT, name);
            let more = if models.len() > 1 { format!("  ·  +{}", models.len() - 1) } else { String::new() };
            c.text(f, k.left(), k.y + 142, 26, false, ACCENT, &format!("{} GB in VRAM{more}", gb(*vram)));
        }
    }
    c.text(f, k.left(), k.y + 212, 30, true, DIM, "CONTAINERS");
    match &s.containers {
        None => {
            c.text(f, k.left(), k.y + 258, 28, false, RED, "docker not readable");
        }
        Some(list) => {
            let running = list.iter().filter(|x| x.1).count();
            let all_up = running == list.len();
            c.text_right(f, k.right(), k.y + 212, 30, true, if all_up { GREEN } else { RED }, &format!("{running}/{}", list.len()));
            let rows = 5;
            // two columns when there are more than fit in one
            let two = list.len() > rows;
            let col_w = if two { k.inner() / 2 } else { k.inner() };
            let shown = if two { rows * 2 } else { rows };
            for (i, (name, up)) in list.iter().take(shown).enumerate() {
                let cx = k.left() + (i / rows) as i32 * col_w;
                let cy = k.y + 258 + (i % rows) as i32 * 38;
                c.rect(cx, cy - 18, 16, 16, 8, if *up { GREEN } else { RED });
                let ns = f.fit(name, 26, false, col_w - 34);
                c.text(f, cx + 28, cy, ns, false, if *up { TEXT } else { RED }, name);
            }
            if list.len() > shown {
                c.text_right(f, k.right(), k.y + 426, 22, false, DIM, &format!("+{} more", list.len() - shown));
            }
        }
    }
    c
}
