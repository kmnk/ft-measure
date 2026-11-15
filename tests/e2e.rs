//! 作った exe を、利用者と同じ入口（名前とメモを標準入力で答える）から、
//! キーだけを台本に替えて通しで動かし、OSC を送って出力を確かめる。

use serde_json::Value;
use std::io::Write;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_ft-measure");

fn percentile_inc(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let h = (v.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(v.len() - 1);
    v[lo] + (h - lo as f64) * (v[hi] - v[lo])
}

fn fmt6(v: f64) -> String {
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.into() }
}

fn find(dir: &Path, suffix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(suffix)).unwrap_or(false)
    })
}

/// chatbox の宛先で受けた文を集める。
fn chatbox_listener(port: u16) -> Arc<Mutex<Vec<String>>> {
    let got = Arc::new(Mutex::new(Vec::new()));
    let sock = UdpSocket::bind(("127.0.0.1", port)).unwrap();
    let g = got.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok((n, _)) = sock.recv_from(&mut buf) {
            if let Ok((_, rosc::OscPacket::Message(m))) = rosc::decoder::decode_udp(&buf[..n]) {
                if m.addr == "/chatbox/input" {
                    if let Some(rosc::OscType::String(s)) = m.args.first() {
                        g.lock().unwrap().push(s.clone());
                    }
                }
            }
        }
    });
    got
}

fn run(dir: &Path, items: &str, script: &str, stdin: &str, port: u16, chat_port: u16) -> String {
    run_lang(dir, items, script, stdin, port, chat_port, "ja")
}

/// `lang` の表示の言語で動かす。
fn run_lang(dir: &Path, items: &str, script: &str, stdin: &str, port: u16, chat_port: u16, lang: &str) -> String {
    let items_path = dir.join("items.txt");
    std::fs::write(&items_path, items).unwrap();
    let script_path = dir.join("script.txt");
    std::fs::write(&script_path, script).unwrap();
    let mut child = Command::new(EXE)
        .args(["--port", &port.to_string(), "--chatbox-port", &chat_port.to_string(), "--chatbox-interval", "0"])
        .arg("--items").arg(&items_path)
        .arg("--out").arg(dir.join("out"))
        .arg("--script").arg(&script_path)
        .arg("--last-file").arg(dir.parent().unwrap().join("last.txt"))
        .arg("--echo-sounds")
        .args(["--lang", lang])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "exe が失敗した: {}", String::from_utf8_lossy(&out.stdout));
    String::from_utf8(out.stdout).unwrap()
}

const P: &str = "/avatar/parameters/FT/In/JawOpen";

#[test]
fn e2e_full_run_last_name_and_kill() {
    let tmp = tempfile::tempdir().unwrap();
    let chat = chatbox_listener(19100);
    let items = "表A\n順に: 向き | 上 | 下\n区間: 読み\n";
    let mut s = String::new();
    // 値が届く前の測定は警告になる
    s += "measure\n";
    s += &format!("send {P} 0.1\nsend /avatar/parameters/GestureLeft 3\nsend /avatar/parameters/FT/InX 0.7\nwait 800\nmeasure\n");
    // 押す直前に値が変わった回は「動いていた」になる
    s += &format!("send {P} 0.5\nwait 150\nmeasure\n");
    s += &format!("send {P} 0.2\nwait 800\nmeasure\n");
    // 4 回目を測って取り消す
    s += &format!("send {P} 0.9\nwait 800\nmeasure\nundo\nnext\n");
    // 順に作る表情: 上 0.9 0.8 0.7、下 0.1 0.2 0.3。上の 2 回目を一度間違えて取り消す
    for v in ["0.9", "0.1"] {
        s += &format!("send {P} {v}\nwait 800\nmeasure\n");
    }
    s += &format!("send {P} 0.0\nwait 800\nmeasure\nundo\n");
    for v in ["0.8", "0.2", "0.7", "0.3"] {
        s += &format!("send {P} {v}\nwait 800\nmeasure\n");
    }
    s += "next\n";
    // 区間: 0.2 を 0.5 秒、0.6 を 0.5 秒
    s += &format!("send {P} 0.2\nwait 300\nmeasure\nwait 500\nsend {P} 0.6\nwait 500\nmeasure\nnext\n");

    let d1 = tmp.path().join("r1");
    std::fs::create_dir_all(&d1).unwrap();
    let stdout = run(&d1, items, &s, "e2e-dev\nメモ, あり\n", 19101, 19100);

    let csv_path = find(&d1.join("out"), "_summary.csv").expect("summary.csv");
    assert!(csv_path.file_name().unwrap().to_str().unwrap().starts_with("e2e-dev_"));
    let csv = std::fs::read_to_string(&csv_path).unwrap();
    let raw = std::fs::read_to_string(find(&d1.join("out"), "_raw.jsonl").unwrap()).unwrap();
    let rows: Vec<Value> = raw.lines().map(|l| serde_json::from_str(l).unwrap()).collect();

    // 1 行目と、測った値の要約
    assert_eq!(rows[0]["type"], "session");
    assert_eq!(rows[0]["device"], "e2e-dev");
    assert_eq!(rows[0]["memo"], "メモ, あり");
    let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
    let m = "e2e-dev,\"メモ, あり\"";
    assert!(lines.contains(&format!("{m},1,表A,pose,JawOpen,3,,0.2,0.1,0.5,,,1").as_str()), "{csv}");
    assert!(lines.contains(&format!("{m},2,向き: 上,pose,JawOpen,3,,0.8,0.7,0.9,,,0").as_str()), "{csv}");
    assert!(lines.contains(&format!("{m},2,向き: 下,pose,JawOpen,3,,0.2,0.1,0.3,,,0").as_str()), "{csv}");

    // 区間の値は、raw の拾いから手で計算した値と一致する
    let samples: Vec<f64> = rows.iter().filter(|r| r["type"] == "sample").map(|r| r["values"]["JawOpen"].as_f64().unwrap()).collect();
    assert!(samples.len() >= 10, "拾いが少ない: {}", samples.len());
    assert!(samples.contains(&0.2) && samples.contains(&0.6));
    let want = format!(
        "{m},3,読み,interval,JawOpen,{},",
        samples.len()
    );
    let line = lines.iter().find(|l| l.starts_with(&want)).unwrap_or_else(|| panic!("区間の行が無い: {csv}"));
    let cols: Vec<&str> = line.split(',').collect();
    // 列: device memo(2 列に見える) item_no item kind key n duration median min max p5 p95 moving_n
    let c = &cols[cols.len() - 8..];
    // c: n duration median min max p5 p95 moving_n
    assert_eq!(c[0], samples.len().to_string());
    let dur: f64 = c[1].parse().unwrap();
    assert!((0.9..1.3).contains(&dur), "区間の長さ {dur}");
    assert_eq!(c[2], fmt6(percentile_inc(samples.clone(), 0.5)));
    assert_eq!(c[3], fmt6(0.2));
    assert_eq!(c[4], fmt6(0.6));
    assert_eq!(c[5], fmt6(percentile_inc(samples.clone(), 0.05)));
    assert_eq!(c[6], fmt6(percentile_inc(samples.clone(), 0.95)));
    assert_eq!(c[7], "");

    // 取り消した回（0.9 の 4 回目、0.0 の向き）と、始めの部分に当たらないものは、どちらにも無い
    let pose_vals: Vec<f64> = rows.iter().filter(|r| r["type"] == "pose").map(|r| r["values"]["JawOpen"].as_f64().unwrap()).collect();
    assert_eq!(pose_vals.len(), 9, "{pose_vals:?}");
    assert!(!rows.iter().any(|r| r["type"] == "pose" && r["item"] == "表A" && r["values"]["JawOpen"] == 0.9));
    assert!(!pose_vals.contains(&0.0));
    for bad in ["GestureLeft", "InX", "/avatar/"] {
        assert!(!csv.contains(bad) && !raw.contains(bad), "{bad} が出ている");
    }

    // 音
    for s in ["測定", "動いていた", "次へ", "取り消し", "警告", "区間の開始", "区間の終わり", "終わり"] {
        assert!(stdout.contains(&format!("[音] {s}")), "音 {s} が無い: {stdout}");
    }

    // chatbox
    std::thread::sleep(Duration::from_millis(200));
    let msgs = chat.lock().unwrap().clone();
    let has = |t: &str| msgs.iter().any(|x| x.contains(t));
    assert!(has("表A 1/3（1/3）"), "{msgs:?}");
    assert!(has("表A 2/3（1/3） 普通の顔に戻してから"), "{msgs:?}");
    assert!(has("値がまだ 1 つも届いていません"), "{msgs:?}");
    assert!(has("動いていた"), "{msgs:?}");
    assert!(msgs.iter().filter(|x| x.contains("向き: 上 2/3（2/3）")).count() >= 2, "取り消しの後に同じ向きが出ていない: {msgs:?}");
    assert!(has("読み 記録中"), "{msgs:?}");
    assert!(has("測定が終わりました"), "{msgs:?}");
    assert!(msgs.iter().all(|x| x.chars().count() <= 144));

    // 前回の名前: 名前を空で答えると、前回の名前になる
    let d2 = tmp.path().join("r2");
    std::fs::create_dir_all(&d2).unwrap();
    run(&d2, items, "quit\n", "\n\n", 19103, 19104);
    let raw2 = std::fs::read_to_string(find(&d2.join("out"), "_raw.jsonl").unwrap()).unwrap();
    let first: Value = serde_json::from_str(raw2.lines().next().unwrap()).unwrap();
    assert_eq!(first["device"], "e2e-dev");

    // 途中で止めても、止める前に測った回は残っている
    let d3 = tmp.path().join("r3");
    std::fs::create_dir_all(&d3).unwrap();
    std::fs::write(d3.join("items.txt"), items).unwrap();
    std::fs::write(d3.join("script.txt"), format!("send {P} 0.4\nwait 800\nmeasure\nwait 20000\n")).unwrap();
    let mut child = Command::new(EXE)
        .args(["--port", "19105", "--chatbox-port", "19106", "--chatbox-interval", "0"])
        .arg("--items").arg(d3.join("items.txt"))
        .arg("--out").arg(d3.join("out"))
        .arg("--script").arg(d3.join("script.txt"))
        .arg("--last-file").arg(tmp.path().join("last.txt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"kill-dev\n\n").unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut seen = false;
    while Instant::now() < end {
        if let Some(p) = find(&d3.join("out"), "_summary.csv") {
            if std::fs::read_to_string(p).unwrap_or_default().contains(",表A,pose,JawOpen,1,") {
                seen = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    let _ = child.wait();
    assert!(seen, "止める前の記録が summary.csv に出なかった");
    let raw3 = std::fs::read_to_string(find(&d3.join("out"), "_raw.jsonl").unwrap()).unwrap();
    assert!(raw3.lines().any(|l| l.contains("\"type\":\"pose\"") && l.contains("0.4")), "{raw3}");
}

#[test]
fn e2e_host_name_does_not_crash() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("s.txt"), "quit\n").unwrap();
    let mut child = Command::new(EXE)
        .args(["--host", "localhost", "--port", "19108", "--chatbox-port", "19109"])
        .arg("--script").arg(tmp.path().join("s.txt"))
        .arg("--out").arg(tmp.path().join("out"))
        .arg("--last-file").arg(tmp.path().join("last.txt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"host-dev\n\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn e2e_port_in_use_is_reported() {
    let _busy = UdpSocket::bind("127.0.0.1:19107").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("s.txt"), "quit\n").unwrap();
    let out = Command::new(EXE)
        .args(["--port", "19107"])
        .arg("--script").arg(tmp.path().join("s.txt"))
        .arg("--out").arg(tmp.path().join("out"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    // 言語を選んでいなければ英語で出す
    assert!(String::from_utf8_lossy(&out.stdout).contains("is used by another app"));
}

#[test]
fn e2e_files_use_english_names_and_display_follows_language() {
    // 名前を「English // 日本語」で書いた一覧で、日本語の表示を選んで測る
    let tmp = tempfile::tempdir().unwrap();
    let items = "Close eyes // 目を閉じる\nsequence: Gaze // 目線 | Up // 上 | Down // 下\n";
    let chat = chatbox_listener(19111);
    let p = "/avatar/parameters/FT/In/EyeClosed";
    let script = format!("send {p} 0.5\nwait 300\nmeasure\nnext\nmeasure\nwait 300\nquit\n");
    let d = tmp.path().join("r");
    std::fs::create_dir_all(&d).unwrap();
    run_lang(&d, items, &script, "lang-dev\n\n", 19110, 19111, "ja");
    let csv = std::fs::read_to_string(find(&d.join("out"), "_summary.csv").unwrap()).unwrap();
    let raw = std::fs::read_to_string(find(&d.join("out"), "_raw.jsonl").unwrap()).unwrap();
    assert!(csv.contains(",Close eyes,pose,") && csv.contains(",Gaze: Up,pose,"), "{csv}");
    assert!(raw.contains("\"item\":\"Gaze: Up\"") && raw.contains("\"direction\":\"Up\""), "{raw}");
    for ja in ["目を閉じる", "目線", "上"] {
        assert!(!csv.contains(ja) && !raw.contains(ja), "{ja} がファイルに出ている");
    }
    std::thread::sleep(Duration::from_millis(200));
    let msgs = chat.lock().unwrap().clone();
    assert!(msgs.iter().any(|x| x.contains("目を閉じる 1/3（1/2）")), "{msgs:?}");
    assert!(msgs.iter().any(|x| x.contains("目線: 下 1/3（2/2）")), "{msgs:?}");

    // 英語の表示では、chatbox も英語の名前になる
    let chat2 = chatbox_listener(19113);
    let d2 = tmp.path().join("r2");
    std::fs::create_dir_all(&d2).unwrap();
    run_lang(&d2, items, &script, "lang-dev\n\n", 19112, 19113, "en");
    std::thread::sleep(Duration::from_millis(200));
    let msgs = chat2.lock().unwrap().clone();
    assert!(msgs.iter().any(|x| x.contains("Close eyes 1/3 (1/2)")), "{msgs:?}");
    assert!(msgs.iter().any(|x| x.contains("Gaze: Down 1/3 (2/2)")), "{msgs:?}");
}
