//! ft-measure: VRChat の OSC の出力から、表情ごとの FT の値を測って書き出す。

mod items;
mod keys;
mod notify;
mod osc;
mod output;
mod session;
mod stats;

use chrono::Local;
use clap::Parser;
use keys::{Input, KeySource, ScriptKeys, TerminalKeys};
use notify::{Chatbox, Sound, SoundPlayer};
use output::{SessionInfo, Writer};
use session::{Config, Event, Session, Snapshot};
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// raw の状態でも崩れないよう、行の終わりを \r\n にして出す。
pub fn say(s: &str) {
    let mut out = io::stdout();
    let _ = write!(out, "{s}\r\n");
    let _ = out.flush();
}

#[derive(Parser, Debug)]
#[command(version, about = "VRChat の OSC の出力から、表情ごとの FT の値を測って書き出す")]
struct Args {
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
    /// 試験用: キーを端末ではなく台本のファイルから読む
    #[arg(long, hide = true)]
    script: Option<PathBuf>,
    /// 試験用: 鳴らした音を画面に出す
    #[arg(long, hide = true)]
    echo_sounds: bool,
    /// 試験用: 前回の名前を残すファイル（既定は exe の隣の ft-measure-last.txt）
    #[arg(long, hide = true)]
    last_file: Option<PathBuf>,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn wait_enter(interactive: bool) {
    if interactive {
        say("Enter で閉じます");
        let mut s = String::new();
        let _ = io::stdin().lock().read_line(&mut s);
    }
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

fn main() {
    let args = Args::parse();
    std::process::exit(run(args));
}

fn run(args: Args) -> i32 {
    let interactive = args.script.is_none();
    let dir = exe_dir();

    // 項目の一覧
    let (items_text, items_label) = match &args.items {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => (t, p.display().to_string()),
            Err(e) => {
                say(&format!("項目の一覧 {} を読めません: {e}", p.display()));
                wait_enter(interactive);
                return 2;
            }
        },
        None => {
            let p = dir.join("items.txt");
            match std::fs::read_to_string(&p) {
                Ok(t) => (t, p.display().to_string()),
                Err(_) => (items::DEFAULT_ITEMS.to_string(), "（組み込みの一覧）".to_string()),
            }
        }
    };
    let item_list = match items::parse(&items_text) {
        Ok(v) => v,
        Err(e) => {
            say(&format!("項目の一覧 {items_label}: {e}"));
            wait_enter(interactive);
            return 2;
        }
    };

    // OSC の受け口
    let socket = match osc::bind(&args.host, args.port) {
        Ok(s) => s,
        Err(e) => {
            if e.kind() == io::ErrorKind::AddrInUse {
                say(&format!("{}:{} は他のアプリが使っています。そのアプリを閉じるか、--port で別のポートにしてください", args.host, args.port));
            } else {
                say(&format!("{}:{} を受けられません: {e}", args.host, args.port));
            }
            wait_enter(interactive);
            return 1;
        }
    };

    say(&format!("ft-measure {}  OSC を {}:{} で受けます", env!("CARGO_PKG_VERSION"), args.host, args.port));
    say(&format!("項目の一覧: {items_label}（{} 項目）", item_list.len()));

    // 名前とメモ（raw の状態にする前に、行で聞く）
    let last_path = args.last_file.clone().unwrap_or_else(|| dir.join("ft-measure-last.txt"));
    let last = std::fs::read_to_string(&last_path).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut device = prompt("名前（例 selfie、vive）", &last);
    if device.is_empty() {
        device = "unnamed".into();
    }
    let _ = std::fs::write(&last_path, format!("{device}\n"));
    let memo = prompt("メモ（空でよい）", "");

    let out_dir = args.out.clone().unwrap_or_else(|| dir.join("ft-measure"));
    let info = SessionInfo {
        device,
        memo,
        started: Local::now(),
        items_file: items_label,
        prefix: args.prefix.clone(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let writer = match Writer::new(&out_dir, info) {
        Ok(w) => w,
        Err(e) => {
            say(&format!("書き出し先 {} を作れません: {e}", out_dir.display()));
            wait_enter(interactive);
            return 2;
        }
    };

    let table: osc::Table = Arc::new(Mutex::new(HashMap::new()));
    let (otx, orx) = mpsc::channel();
    osc::spawn(socket, args.prefix.clone(), table.clone(), otx);

    let sounds = SoundPlayer::new(args.echo_sounds);
    let mut chat = if args.no_chatbox {
        None
    } else {
        // chatbox は VRChat の入力（同じ PC の 127.0.0.1）へ送る。受け口の --host とは別
        let dest = std::net::SocketAddr::from(([127, 0, 0, 1], args.chatbox_port));
        Chatbox::new(dest, Duration::from_secs_f64(args.chatbox_interval.max(0.0))).ok()
    };

    let mut session = Session::new(item_list, Config { reps: args.reps.max(1), move_threshold: args.move_threshold });
    if let Err(e) = writer.write_all(&session) {
        say(&format!("書き出せません: {e}"));
    }
    say(&format!("書き出し先: {}", writer.summary_path.display()));
    say("スペース: 測定　Enter: 次へ　Backspace: 取り消し　Ctrl+C: 終わる");

    let mut source: Box<dyn KeySource> = match &args.script {
        Some(p) => {
            let text = std::fs::read_to_string(p).unwrap_or_default();
            let target = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
            match ScriptKeys::parse(&text, target) {
                Ok(s) => Box::new(s),
                Err(e) => {
                    say(&e);
                    return 2;
                }
            }
        }
        None => match TerminalKeys::new() {
            Ok(t) => Box::new(t),
            Err(e) => {
                say(&format!("キーを読めません: {e}"));
                wait_enter(true);
                return 2;
            }
        },
    };

    let status = session.status();
    say(&status);
    if let Some(c) = chat.as_mut() {
        c.set(&status, Instant::now());
    }

    let t0 = Instant::now();
    let dt = Duration::from_secs_f64(1.0 / args.sample_hz.max(1.0));
    let mut next_sample = t0;
    let mut recent: VecDeque<(f64, Snapshot)> = VecDeque::new();
    let snapshot = |table: &osc::Table| -> Snapshot { table.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect() };

    let mut quit = false;
    while !quit && !session.finished {
        let now = Instant::now();
        let inputs = source.poll(next_sample.saturating_duration_since(now));

        while let Ok(osc::OscEvent::AvatarChanged(_)) = orx.try_recv() {
            recent.clear();
            say("アバターが変わりました。それまでの値を捨てました");
            if let Some(c) = chat.as_mut() {
                c.set(&format!("アバターが変わった {}", session.status()), Instant::now());
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
                    Event::Recorded { label, rep, of, moving, keys } => {
                        sounds.play(Sound::Measure);
                        say(&format!("記録: {label} {rep}/{of}（{keys} キー）"));
                        if *moving {
                            sounds.play(Sound::Moving);
                            say("動いていました。取り消して測り直せます");
                            note = "動いていた 取り消せます / ".into();
                        }
                    }
                    Event::Warn(m) => {
                        sounds.play(Sound::Warn);
                        say(m);
                        note = format!("{m} / ");
                    }
                    Event::Undone { label } => {
                        sounds.play(Sound::Undo);
                        say(&format!("取り消し: {label}"));
                    }
                    Event::IntervalStarted { restarted } => {
                        sounds.play(Sound::IntervalStart);
                        say(if *restarted { "区間をやり直します（前の区間は捨てました）" } else { "区間の記録を始めました" });
                    }
                    Event::IntervalStopped { samples, duration } => {
                        sounds.play(Sound::IntervalStop);
                        say(&format!("区間の記録を止めました（{samples} 回、{duration:.1} 秒）"));
                    }
                    Event::IntervalDiscarded => {
                        sounds.play(Sound::Undo);
                        say("区間を取り消しました");
                    }
                    Event::Advanced => sounds.play(Sound::Next),
                    Event::Finished => sounds.play(Sound::Finish),
                    Event::Changed => {
                        if let Err(e) = writer.write_all(&session) {
                            say(&format!("書き出せません: {e}"));
                        }
                    }
                }
            }
            let status = session.status();
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

    if let Err(e) = writer.write_all(&session) {
        say(&format!("書き出せません: {e}"));
    }
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
    say(&format!("書き出しました: {}", writer.summary_path.display()));
    say(&format!("            : {}", writer.raw_path.display()));
    if session.finished {
        // 終わりの音を鳴らし切る
        std::thread::sleep(Duration::from_millis(600));
        wait_enter(interactive);
    }
    0
}
