//! 測っている間の様子。測るスレッドが書き、窓が読む。窓を出さない試験では、標準出力にも出す。

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::session::Progress;

/// 窓に残す行の数。
const LOG_LINES: usize = 200;

/// 行の種類。窓で色を分ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warn,
}

#[derive(Default, Clone)]
pub struct View {
    pub log: VecDeque<(Level, String)>,
    /// 今の項目と回
    pub progress: Progress,
    pub summary_path: Option<PathBuf>,
    pub raw_path: Option<PathBuf>,
    /// 測るスレッドが終わった。Some(true) は一覧の最後まで進んだ
    pub ended: Option<bool>,
}

static VIEW: Mutex<Option<View>> = Mutex::new(None);
static REPAINT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// 様子が変わったときに窓を描き直させる。
pub fn on_change(f: Box<dyn Fn() + Send + Sync>) {
    let _ = REPAINT.set(f);
}

fn update(f: impl FnOnce(&mut View)) {
    {
        let mut g = VIEW.lock().unwrap_or_else(|e| e.into_inner());
        f(g.get_or_insert_with(View::default));
    }
    if let Some(r) = REPAINT.get() {
        r();
    }
}

pub fn snapshot() -> View {
    VIEW.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// 1 行を出す。
pub fn say(s: &str) {
    log(Level::Info, s);
}

pub fn log(level: Level, s: &str) {
    let mut out = io::stdout();
    let _ = writeln!(out, "{s}");
    let _ = out.flush();
    update(|v| {
        v.log.push_back((level, s.to_string()));
        while v.log.len() > LOG_LINES {
            v.log.pop_front();
        }
    });
}

/// 今の項目と回を窓に出す。
pub fn set_progress(p: Progress) {
    update(|v| v.progress = p);
}

pub fn set_paths(summary: &Path, raw: &Path) {
    update(|v| {
        v.summary_path = Some(summary.to_path_buf());
        v.raw_path = Some(raw.to_path_buf());
    });
}

pub fn set_ended(finished: bool) {
    update(|v| v.ended = Some(finished));
}
