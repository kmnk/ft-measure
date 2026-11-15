//! ft-measure: VRChat の OSC の出力から、表情ごとの FT の値を測って書き出す。

// Windows でダブルクリックしたときに、黒いコンソールの窓を出さない
#![cfg_attr(windows, windows_subsystem = "windows")]

mod gui;
mod i18n;
mod items;
mod keys;
mod notify;
mod osc;
mod output;
mod session;
mod stats;
mod view;

use chrono::Local;
use clap::Parser;
use i18n::{tr, Lang};
use keys::{Input, KeySource, ScriptKeys};
use notify::{Chatbox, Sound, SoundPlayer};
use output::{SessionInfo, Writer};
use session::{Config, Event, Session, Snapshot};
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use view::Level;

pub use view::say;

#[derive(Parser, Debug, Clone)]
#[command(version, about = "Record VRChat face tracking values per expression from OSC")]
pub struct Args {
    /// OSC を受けるアドレス
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// OSC を受けるポート（VRChat の出力）
    #[arg(long, default_value_t = 9001)]
    port: u16,
    /// chatbox を送るポート（VRChat の入力）
    #[arg(long, default_value_t = 9000)]
    chatbox_port: u16,
    /// 測るパラメータの始めの部分（/avatar/parameters/ の後）
    #[arg(long, default_value = "FT/In/")]
    prefix: String,
    /// 項目の一覧のファイル（既定は exe の隣の items.txt、無ければ組み込みの一覧）
    #[arg(long)]
    items: Option<PathBuf>,
    /// 書き出し先のディレクトリ（既定は exe の隣の ft-measure）
    #[arg(long)]
    out: Option<PathBuf>,
    /// 1 つの表情（向き）を測る回数
    #[arg(long, default_value_t = 3)]
    reps: usize,
    /// 1 秒に値を拾う回数
    #[arg(long, default_value_t = 20.0)]
    sample_hz: f64,
    /// 「動いていた」を見る、測定の直前の秒数
    #[arg(long, default_value_t = 0.5)]
    move_window: f64,
    /// 「動いていた」とみなす、最大と最小の差
    #[arg(long, default_value_t = 0.05)]
    move_threshold: f64,
    /// chatbox に送る最短の間隔（秒）
    #[arg(long, default_value_t = 1.5)]
    chatbox_interval: f64,
    /// chatbox に送らない
    #[arg(long)]
    no_chatbox: bool,
    /// 表示の言語（en または ja）。既定は前回の選択、無ければ en
    #[arg(long)]
    lang: Option<String>,
    /// 試験用: 窓を出さず、キーを台本のファイルから、名前とメモを標準入力から読む
    #[arg(long, hide = true)]
    script: Option<PathBuf>,
    /// 試験用: 窓を出したまま、キーを台本のファイルから読み、前回の名前で始める
    #[arg(long, hide = true)]
    window_script: Option<PathBuf>,
    /// 試験用: 鳴らした音を画面に出す
    #[arg(long, hide = true)]
    echo_sounds: bool,
    /// 試験用: 前回の名前を残すファイル（既定は exe の隣の ft-measure-last.txt）
    #[arg(long, hide = true)]
    last_file: Option<PathBuf>,
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn prompt(label: &str, default: &str) -> String {
    if default.is_empty() {
        print!("{label}: ");
    } else {
        print!("{label} [{default}]: ");
    }
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().lock().read_line(&mut s);
    let s = s.trim();
    if s.is_empty() { default.to_string() } else { s.to_string() }
}

/// 前回の名前と言語。exe の隣の ft-measure-last.txt に `name=` と `lang=` の行で残す。
/// 古い版の、名前だけの 1 行も読む。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Saved {
    pub name: String,
    pub lang: Option<Lang>,
}

impl Saved {
    pub fn parse(text: &str) -> Saved {
        let mut s = Saved::default();
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name=") {
                s.name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("lang=") {
                s.lang = Lang::from_code(v);
            } else if !line.is_empty() && s.name.is_empty() {
                s.name = line.to_string();
            }
        }
        s
    }

    pub fn load(path: &Path) -> Saved {
        Saved::parse(&std::fs::read_to_string(path).unwrap_or_default())
    }

    pub fn store(&self, path: &Path) {
        let lang = self.lang.map(|l| format!("lang={}\n", l.code())).unwrap_or_default();
        let _ = std::fs::write(path, format!("name={}\n{lang}", self.name));
    }
}

fn last_path(args: &Args) -> PathBuf {
    args.last_file.clone().unwrap_or_else(|| exe_dir().join("ft-measure-last.txt"))
}

fn main() {
    let args = Args::parse();
    // 言語は、指定 → 前回の選択 → 英語 の順に決める
    let saved = Saved::load(&last_path(&args));
    let lang = args.lang.as_deref().and_then(Lang::from_code).or(saved.lang).unwrap_or(Lang::En);
    i18n::set_lang(lang);
    let code = if args.script.is_some() { run_script(args) } else { gui::run(args) };
    std::process::exit(code);
}

/// 測り始める前に用意するもの。OSC は、名前を聞いている間から受け始め、値が届いているかを窓に出す。
pub struct Prepared {
    pub items: Vec<items::Item>,
    pub items_label: String,
    pub table: osc::Table,
    pub osc_events: mpsc::Receiver<osc::OscEvent>,
    pub last_path: PathBuf,
    pub saved: Saved,
    pub out_dir: PathBuf,
}

/// 用意の誤り。窓を出さない試験では終わりの番号を変える。
pub struct PrepareError {
    pub message: String,
    pub code: i32,
}

pub fn prepare(args: &Args) -> Result<Prepared, PrepareError> {
    let dir = exe_dir();
    let items_err = |message: String| PrepareError { message, code: 2 };
    let (items_text, items_label) = match &args.items {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => (t, p.display().to_string()),
            Err(e) => return Err(items_err(format!("{} {}: {e}", tr("Cannot read the item list", "項目の一覧を読めません:"), p.display()))),
        },
        None => {
            let p = dir.join("items.txt");
            match std::fs::read_to_string(&p) {
                Ok(t) => (t, p.display().to_string()),
                Err(_) => (items::DEFAULT_ITEMS.to_string(), "(built-in)".to_string()),
            }
        }
    };
    let items = items::parse(&items_text).map_err(|e| items_err(format!("{} {items_label}: {e}", tr("Item list", "項目の一覧"))))?;
    let socket = osc::bind(&args.host, args.port).map_err(|e| {
        let message = if e.kind() == io::ErrorKind::AddrInUse {
            match i18n::lang() {
                Lang::En => format!("Port {}:{} is used by another app. Close that app (for example another OSC tool) and start again.", args.host, args.port),
                Lang::Ja => format!("{}:{} は他のアプリが使っています。そのアプリ（他の OSC の道具など）を閉じてから、もう一度開いてください", args.host, args.port),
            }
        } else {
            format!("{} {}:{}: {e}", tr("Cannot receive OSC on", "OSC を受けられません"), args.host, args.port)
        };
        PrepareError { message, code: 1 }
    })?;
    let table: osc::Table = Arc::new(Mutex::new(HashMap::new()));
    let (otx, orx) = mpsc::channel();
    osc::spawn(socket, args.prefix.clone(), table.clone(), otx);
    let last_path = last_path(args);
    let saved = Saved::load(&last_path);
    let out_dir = args.out.clone().unwrap_or_else(|| dir.join("ft-measure"));
    Ok(Prepared { items, items_label, table, osc_events: orx, last_path, saved, out_dir })
}

/// 窓を出さずに、台本のキーで測る（試験用）。
fn run_script(args: Args) -> i32 {
    let prep = match prepare(&args) {
        Ok(p) => p,
        Err(e) => {
            say(&e.message);
            return e.code;
        }
    };
    let device = prompt(tr("Name (e.g. selfie, vive)", "名前（例 selfie、vive）"), &prep.saved.name);
    let memo = prompt(tr("Memo (optional)", "メモ（空でよい）"), "");
    let text = std::fs::read_to_string(args.script.as_ref().unwrap()).unwrap_or_default();
    let target = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    let source = match ScriptKeys::parse(&text, target) {
        Ok(s) => s,
        Err(e) => {
            say(&e);
            return 2;
        }
    };
    match measure(&args, prep, device, memo, Box::new(source)) {
        Ok(()) => 0,
        Err(e) => {
            say(&e);
            2
        }
    }
}

/// 名前とメモを決めてから、一覧の終わりか「終わる」の入力まで測る。窓では別のスレッドで動く。
pub fn measure(args: &Args, prep: Prepared, device: String, memo: String, mut source: Box<dyn KeySource>) -> Result<(), String> {
    let device = if device.trim().is_empty() { "unnamed".to_string() } else { device.trim().to_string() };
    Saved { name: device.clone(), lang: Some(i18n::lang()) }.store(&prep.last_path);

    say(&format!("ft-measure {}  OSC {}:{}", env!("CARGO_PKG_VERSION"), args.host, args.port));
    say(&format!("{}: {} ({})", tr("Item list", "項目の一覧"), prep.items_label, prep.items.len()));

    let info = SessionInfo {
        device,
        memo: memo.trim().to_string(),
        started: Local::now(),
        items_file: prep.items_label.clone(),
        prefix: args.prefix.clone(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let writer = Writer::new(&prep.out_dir, info)
        .map_err(|e| format!("{} {}: {e}", tr("Cannot create the output folder", "書き出し先を作れません:"), prep.out_dir.display()))?;
    view::set_paths(&writer.summary_path, &writer.raw_path);

    let table = prep.table;
    let orx = prep.osc_events;
    // 名前を聞いている間に届いたアバターの変更は、測る前のことなので読み捨てる
    while orx.try_recv().is_ok() {}

    let sounds = SoundPlayer::new(args.echo_sounds);
    let mut chat = if args.no_chatbox {
        None
    } else {
        // chatbox は VRChat の入力（同じ PC の 127.0.0.1）へ送る。受け口の --host とは別
        let dest = std::net::SocketAddr::from(([127, 0, 0, 1], args.chatbox_port));
        Chatbox::new(dest, Duration::from_secs_f64(args.chatbox_interval.max(0.0))).ok()
    };

    let mut session = Session::new(prep.items, Config { reps: args.reps.max(1), move_threshold: args.move_threshold });
    let write = |session: &Session| {
        if let Err(e) = writer.write_all(session) {
            view::log(Level::Warn, &format!("{}: {e}", tr("Cannot write the files", "書き出せません")));
        }
    };
    write(&session);
    say(&format!("{}: {}", tr("Output", "書き出し先"), writer.summary_path.display()));

    let show = |session: &Session| {
        view::set_progress(session.progress());
        session.status()
    };
    let status = show(&session);
    say(&status);
    if let Some(c) = chat.as_mut() {
        c.set(&status, Instant::now());
    }

    let t0 = Instant::now();
    let dt = Duration::from_secs_f64(1.0 / args.sample_hz.max(1.0));
    let mut next_sample = t0;
    let mut recent: VecDeque<(f64, Snapshot)> = VecDeque::new();
    let snapshot = |table: &osc::Table| -> Snapshot { table.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect() };
    let mut shown_lang = i18n::lang();

    let mut quit = false;
    while !quit && !session.finished {
        let now = Instant::now();
        let inputs = source.poll(next_sample.saturating_duration_since(now));

        while let Ok(osc::OscEvent::AvatarChanged(_)) = orx.try_recv() {
            recent.clear();
            view::log(Level::Warn, tr("The avatar changed. Values received before it were discarded", "アバターが変わりました。それまでの値を捨てました"));
            if let Some(c) = chat.as_mut() {
                c.set(&format!("{} {}", tr("Avatar changed /", "アバターが変わった"), session.status()), Instant::now());
            }
        }

        // 窓で言語を変えたら、chatbox もその言語で出し直す
        if i18n::lang() != shown_lang {
            shown_lang = i18n::lang();
            let status = show(&session);
            if let Some(c) = chat.as_mut() {
                c.set(&status, Instant::now());
            }
        }

        for input in inputs {
            let key = match input {
                Input::Quit => {
                    quit = true;
                    break;
                }
                Input::Key(k) => k,
            };
            let t = t0.elapsed().as_secs_f64();
            let snap = snapshot(&table);
            let recent_list: Vec<Snapshot> = recent.iter().map(|(_, s)| s.clone()).collect();
            let events = session.handle_key(key, t, Local::now(), &snap, &recent_list);
            let mut note = String::new();
            for ev in &events {
                match ev {
                    Event::Recorded { label, rep, of, keys, .. } => {
                        // 「動いていた」は記録に残すだけで、知らせない。顔の追跡の値は止めているつもりでも
                        // ほぼ毎回しきい値を超え、知らせが出続けたため（2026-11-15）
                        sounds.play(Sound::Measure);
                        let rec = match i18n::lang() {
                            Lang::En => format!("Recorded: {label} {rep}/{of} ({keys} {})", if *keys == 1 { "key" } else { "keys" }),
                            Lang::Ja => format!("記録しました: {label} {rep}/{of}（{keys} キー）"),
                        };
                        view::log(Level::Ok, &rec);
                    }
                    Event::Warn(m) => {
                        sounds.play(Sound::Warn);
                        view::log(Level::Warn, m);
                        note = format!("{m} / ");
                    }
                    Event::Undone { label } => {
                        sounds.play(Sound::Undo);
                        say(&format!("{}: {label}", tr("Undone", "取り消し")));
                    }
                    Event::IntervalStarted { restarted } => {
                        sounds.play(Sound::IntervalStart);
                        say(if *restarted {
                            tr("Recording restarted (the previous recording was discarded)", "区間をやり直します（前の区間は捨てました）")
                        } else {
                            tr("Recording started", "区間の記録を始めました")
                        });
                    }
                    Event::IntervalStopped { samples, duration } => {
                        sounds.play(Sound::IntervalStop);
                        let m = match i18n::lang() {
                            Lang::En => format!("Recording stopped ({samples} samples, {duration:.1} s)"),
                            Lang::Ja => format!("区間の記録を止めました（{samples} 回、{duration:.1} 秒）"),
                        };
                        view::log(Level::Ok, &m);
                    }
                    Event::IntervalDiscarded => {
                        sounds.play(Sound::Undo);
                        say(tr("Recording undone", "区間を取り消しました"));
                    }
                    Event::Advanced => sounds.play(Sound::Next),
                    Event::Finished => sounds.play(Sound::Finish),
                    Event::Changed => write(&session),
                }
            }
            let status = show(&session);
            say(&status);
            if let Some(c) = chat.as_mut() {
                c.set(&format!("{note}{status}"), Instant::now());
            }
            if session.finished {
                break;
            }
        }

        let now = Instant::now();
        if now >= next_sample {
            let t = t0.elapsed().as_secs_f64();
            let snap = snapshot(&table);
            session.on_sample(t, Local::now(), &snap);
            recent.push_back((t, snap));
            while let Some((tt, _)) = recent.front() {
                if t - *tt > args.move_window {
                    recent.pop_front();
                } else {
                    break;
                }
            }
            next_sample += dt;
            if next_sample < now {
                next_sample = now + dt;
            }
        }
        if let Some(c) = chat.as_mut() {
            c.tick(Instant::now());
        }
    }

    write(&session);
    // chatbox に最後の文を送り切る
    if let Some(c) = chat.as_mut() {
        let end = Instant::now() + Duration::from_secs_f64(args.chatbox_interval.max(0.0)) + Duration::from_millis(50);
        while Instant::now() < end {
            c.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(20));
        }
        c.tick(Instant::now());
    }
    drop(source);
    view::log(Level::Ok, &format!("{}: {}", tr("Saved", "書き出しました"), writer.summary_path.display()));
    say(&format!("            : {}", writer.raw_path.display()));
    if session.finished {
        // 終わりの音を鳴らし切る
        std::thread::sleep(Duration::from_millis(600));
    }
    view::set_ended(session.finished);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_reads_new_and_old_forms() {
        assert_eq!(Saved::parse("vive\n"), Saved { name: "vive".into(), lang: None });
        assert_eq!(Saved::parse("name=selfie\nlang=ja\n"), Saved { name: "selfie".into(), lang: Some(Lang::Ja) });
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("last.txt");
        let s = Saved { name: "a b".into(), lang: Some(Lang::En) };
        s.store(&p);
        assert_eq!(Saved::load(&p), s);
    }
}
