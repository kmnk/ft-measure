//! 作った exe を、利用者と同じ入口（名前とメモを標準入力で答える）から、
//! キーだけを台本に替えて通しで動かし、OSC を送って出力を確かめる。
//! 項目の一覧は exe に組み込みのものだけなので、それを最後まで進める。

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_ft-measure");

/// 組み込みの一覧（spec.md の「項目の `id`」の表）。種類と、書き出す行の英語の名前と id
enum Kind {
    Pose,
    Sequence,
    Interval,
}
const ITEMS: &[(Kind, &[(&str, &str)])] = &[
    (Kind::Pose, &[("Close both eyes tightly", "eyes_close")]),
    (Kind::Pose, &[("Open both eyes as wide as you can", "eyes_wide")]),
    (Kind::Pose, &[("Squint by raising the lower eyelids (as in bright light)", "eyes_squint")]),
    (Kind::Pose, &[("Raise both brows as high as you can (surprise)", "brows_up")]),
    (Kind::Pose, &[("Pull the brows together and down (frown)", "brows_down")]),
    (Kind::Pose, &[("Open the mouth vertically as far as you can", "jaw_open")]),
    (Kind::Pose, &[("With the mouth open, stretch the lips sideways", "mouth_stretch")]),
    (Kind::Pose, &[("With the mouth closed, pull the corners out and up", "mouth_smile")]),
    (Kind::Pose, &[("With the mouth closed, pull the corners down", "mouth_frown")]),
    (Kind::Pose, &[("Pucker the lips and push them forward", "lips_pucker")]),
    (Kind::Pose, &[("Bite the teeth together and open the lips (\"eee\")", "lips_apart")]),
    (Kind::Pose, &[("Close the mouth and puff out the cheeks", "cheeks_puff")]),
    (Kind::Pose, &[("Stick the tongue out as far as you can", "tongue_out")]),
    (Kind::Sequence, &[("Wink: Close the right eye only", "wink_right"), ("Wink: Close the left eye only", "wink_left")]),
    (
        Kind::Sequence,
        &[
            ("Gaze (face forward): Up", "gaze_up"),
            ("Gaze (face forward): Down", "gaze_down"),
            ("Gaze (face forward): Right", "gaze_right"),
            ("Gaze (face forward): Left", "gaze_left"),
        ],
    ),
    (
        Kind::Sequence,
        &[("Lips: Press the lips together hard (\"mmm\")", "lips_press"), ("Lips: Roll the lips into the mouth to hide them", "lips_roll")],
    ),
    (Kind::Sequence, &[("Jaw (mouth slightly open): Shift right", "jaw_right"), ("Jaw (mouth slightly open): Shift left", "jaw_left")]),
    (
        Kind::Sequence,
        &[
            ("Tongue out, point the tip: Up", "tongue_up"),
            ("Tongue out, point the tip: Down", "tongue_down"),
            ("Tongue out, point the tip: Right", "tongue_right"),
            ("Tongue out, point the tip: Left", "tongue_left"),
        ],
    ),
    (Kind::Interval, &[("Read aloud with a neutral face", "read_neutral")]),
    (Kind::Interval, &[("Read aloud with a light smile", "read_smile")]),
    (Kind::Pose, &[("Relaxed neutral face", "neutral")]),
];

const P: &str = "/avatar/parameters/FT/In/JawOpen";
/// 送ってから測るまでの待ち（ミリ秒）。値が届くのを待つだけ
const WAIT: u32 = 50;

/// id ごとに違う値にして、行の id が測った項目と合うかを値で見る。`rep` は 0 から
fn value_of(k: usize, rep: usize) -> f64 {
    ((k + 1) * 10 + rep) as f64 / 1000.0
}

/// 一覧を最後まで進める台本と、id ごとに測るはずの値。
/// 最初の項目では、値が届く前に測り、3 回そろう前に次へを押し、4 回目を測って取り消す。
/// 最初の順に作る表情（ウィンク）では、2 周目の右目の前に 0.0 を測って取り消す。
fn full_script() -> (String, BTreeMap<String, Vec<f64>>) {
    let mut s = String::from("measure\n");
    s += "send /avatar/parameters/GestureLeft 3\nsend /avatar/parameters/FT/InX 0.7\n";
    let mut want: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut k = 0;
    for (n, (kind, rows)) in ITEMS.iter().enumerate() {
        match kind {
            Kind::Pose => {
                let id = rows[0].1;
                for rep in 0..3 {
                    let v = value_of(k, rep);
                    s += &format!("send {P} {v}\nwait {WAIT}\nmeasure\n");
                    want.entry(id.into()).or_default().push(v);
                    if n == 0 && rep == 0 {
                        s += "next\n";
                    }
                }
                if n == 0 {
                    s += &format!("send {P} 0.999\nwait {WAIT}\nmeasure\nundo\n");
                }
                k += 1;
            }
            Kind::Sequence => {
                // 向きは 3 周する
                for rep in 0..3 {
                    if n == 13 && rep == 1 {
                        s += &format!("send {P} 0.0\nwait {WAIT}\nmeasure\nundo\n");
                    }
                    for (d, (_, id)) in rows.iter().enumerate() {
                        let v = value_of(k + d, rep);
                        s += &format!("send {P} {v}\nwait {WAIT}\nmeasure\n");
                        want.entry((*id).into()).or_default().push(v);
                    }
                }
                k += rows.len();
            }
            Kind::Interval => {
                let (a, b) = (value_of(k, 0), value_of(k, 1));
                s += &format!("send {P} {a}\nwait {WAIT}\nmeasure\nwait 300\nsend {P} {b}\nwait 300\nmeasure\n");
                want.insert(rows[0].1.into(), vec![a, b]);
                k += 1;
            }
        }
        s += "next\n";
    }
    (s, want)
}

fn find(dir: &Path, suffix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(suffix)).unwrap_or(false)
    })
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn read_rows(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
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

/// `exe` を `lang` の表示の言語で、台本のキーで動かす。
fn run_exe(exe: &Path, dir: &Path, script: &str, stdin: &str, port: u16, chat_port: u16, lang: &str) -> String {
    let script_path = dir.join("script.txt");
    std::fs::write(&script_path, script).unwrap();
    let mut child = Command::new(exe)
        .args(["--port", &port.to_string(), "--chatbox-port", &chat_port.to_string(), "--chatbox-interval", "0"])
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

fn run_lang(dir: &Path, script: &str, stdin: &str, port: u16, chat_port: u16, lang: &str) -> String {
    run_exe(Path::new(EXE), dir, script, stdin, port, chat_port, lang)
}

#[test]
fn e2e_full_run_last_name_and_kill() {
    let tmp = tempfile::tempdir().unwrap();
    let chat = chatbox_listener(19100);
    let (script, want) = full_script();
    let d1 = tmp.path().join("r1");
    std::fs::create_dir_all(&d1).unwrap();
    let stdout = run_lang(&d1, &script, "e2e-dev\nメモ, あり\n", 19101, 19100, "ja");

    // 書き出すのは <名前>_<日時>.jsonl の 1 つだけ
    let names = files(&d1.join("out"));
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("e2e-dev_") && names[0].ends_with(".jsonl") && !names[0].contains("_raw"), "{names:?}");
    assert!(find(&d1.join("out"), "_summary.csv").is_none());
    let path = d1.join("out").join(&names[0]);
    let raw = std::fs::read_to_string(&path).unwrap();
    let rows = read_rows(&path);

    // 1 行目
    assert_eq!(rows[0]["type"], "session");
    assert_eq!(rows[0]["format_version"], 1);
    assert_eq!(rows[0]["device"], "e2e-dev");
    assert_eq!(rows[0]["memo"], "メモ, あり");
    assert_eq!(rows[0]["prefix"], "FT/In/");
    assert!(rows[0].get("items_file").is_none(), "{}", rows[0]);

    // 2 行目からの全部の行が id を持ち、項目の名前と id の組が表と一致する。moving は無く、pose の行は spread を持つ
    let table: BTreeMap<&str, &str> = ITEMS.iter().flat_map(|(_, r)| r.iter().map(|(n, id)| (*id, *n))).collect();
    assert_eq!(table.len(), 30);
    let mut seen: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for r in &rows[1..] {
        let id = r["id"].as_str().unwrap_or_else(|| panic!("id が無い: {r}"));
        assert_eq!(table.get(id).copied(), r["item"].as_str(), "{r}");
        assert!(r.get("moving").is_none(), "{r}");
        if r["type"] == "pose" {
            assert!(r["spread"].is_object(), "{r}");
            seen.entry(id.into()).or_default().push(r["values"]["JawOpen"].as_f64().unwrap());
        }
    }
    // 測った値が、その id の項目で送った値と一致する（取り消した 4 回目の 0.999 は残らない）
    for (id, vals) in &want {
        if ["read_neutral", "read_smile"].contains(&id.as_str()) {
            continue;
        }
        let mut got = seen.get(id).cloned().unwrap_or_default();
        let mut w = vals.clone();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        w.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(got, w, "{id}");
    }
    assert_eq!(seen.len(), 28, "{:?}", seen.keys());
    // 文字列ではなく値で見る（時刻の文字列が 0.999 を含むことがある）
    assert!(!rows.iter().any(|r| r["values"]["JawOpen"].as_f64() == Some(0.999)), "取り消した回が残っている");
    assert!(!rows.iter().any(|r| r["values"]["JawOpen"].as_f64() == Some(0.0)), "順に作る表情で取り消した回が残っている");
    // 順に作る表情の行は、英語の向きの名前を direction に持つ
    assert!(rows.iter().filter(|r| r["id"] == "gaze_up").all(|r| r["direction"] == "Up" && r["item"] == "Gaze (face forward): Up"));
    assert_eq!(rows.iter().filter(|r| r["id"] == "gaze_up").count(), 3);

    // 区間の拾いは、その区間で送った値を持つ
    for id in ["read_neutral", "read_smile"] {
        let samples: Vec<f64> = rows.iter().filter(|r| r["type"] == "sample" && r["id"] == id).map(|r| r["values"]["JawOpen"].as_f64().unwrap()).collect();
        assert!(samples.len() >= 8, "{id} の拾いが少ない: {}", samples.len());
        for v in &want[id] {
            assert!(samples.contains(v), "{id} に {v} が無い: {samples:?}");
        }
    }

    // 始めの部分に当たらないものは出ない
    for bad in ["GestureLeft", "InX", "/avatar/"] {
        assert!(!raw.contains(bad), "{bad} が出ている");
    }

    // 音
    for s in ["測定", "次へ", "取り消し", "警告", "区間の開始", "区間の終わり", "終わり"] {
        assert!(stdout.contains(&format!("[音] {s}")), "音 {s} が無い: {stdout}");
    }
    assert!(!stdout.contains("動いていた"), "動いていたの知らせが出ている: {stdout}");

    // chatbox
    std::thread::sleep(Duration::from_millis(200));
    let msgs = chat.lock().unwrap().clone();
    let has = |t: &str| msgs.iter().any(|x| x.contains(t));
    assert!(has("両目をぎゅっと閉じる 1/3（1/21）"), "{msgs:?}");
    assert!(has("両目をぎゅっと閉じる 2/3（1/21） 顔をリラックスさせてから、もう一度"), "{msgs:?}");
    assert!(has("値がまだ 1 つも届いていません"), "{msgs:?}");
    assert!(has("あと 2 回測ってから次へ進めます"), "{msgs:?}");
    assert!(has("ウィンク: 右目だけ閉じる 1/3（14/21）"), "{msgs:?}");
    // 取り消しの後は、消した回の向きがもう一度知らされる
    assert!(msgs.iter().filter(|x| x.contains("ウィンク: 右目だけ閉じる 2/3（14/21）")).count() >= 2, "取り消しの後に同じ向きが出ていない: {msgs:?}");
    assert!(has("無表情で読み上げ 測定で始める（19/21） 例: あめんぼ赤いな"), "{msgs:?}");
    assert!(has("無表情で読み上げ 記録中"), "{msgs:?}");
    assert!(has("測定が終わりました"), "{msgs:?}");
    assert!(!has("動いていた"), "{msgs:?}");
    assert!(msgs.iter().all(|x| x.chars().count() <= 144));
    // ファイルには日本語の名前が出ない（メモは書いたとおり）
    for ja in ["両目", "ウィンク", "読み上げ"] {
        assert!(!raw.contains(ja), "{ja} がファイルに出ている");
    }

    // 前回の名前: 名前を空で答えると、前回の名前になる
    let d2 = tmp.path().join("r2");
    std::fs::create_dir_all(&d2).unwrap();
    run_lang(&d2, "quit\n", "\n\n", 19103, 19104, "ja");
    let rows2 = read_rows(&find(&d2.join("out"), ".jsonl").unwrap());
    assert_eq!(rows2[0]["device"], "e2e-dev");

    // 途中で止めても、止める前に測った回は残っている
    let d3 = tmp.path().join("r3");
    std::fs::create_dir_all(&d3).unwrap();
    std::fs::write(d3.join("script.txt"), format!("send {P} 0.4\nwait {WAIT}\nmeasure\nwait 20000\n")).unwrap();
    let mut child = Command::new(EXE)
        .args(["--port", "19105", "--chatbox-port", "19106", "--chatbox-interval", "0"])
        .arg("--out").arg(d3.join("out"))
        .arg("--script").arg(d3.join("script.txt"))
        .arg("--last-file").arg(tmp.path().join("last.txt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"kill-dev\n\n").unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut seen_pose = false;
    while Instant::now() < end {
        if let Some(p) = find(&d3.join("out"), ".jsonl") {
            let t = std::fs::read_to_string(p).unwrap_or_default();
            let pose = t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).any(|r| {
                r["type"] == "pose" && r["id"] == "eyes_close" && r["values"]["JawOpen"].as_f64() == Some(0.4)
            });
            if pose {
                seen_pose = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().unwrap();
    let _ = child.wait();
    assert!(seen_pose, "止める前の記録がファイルに出なかった");
}

#[test]
fn e2e_items_file_next_to_exe_is_ignored() {
    // exe を一時のディレクトリに写し、隣に項目を書き換えた items.txt を置いても、組み込みの一覧で測る
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join(Path::new(EXE).file_name().unwrap());
    std::fs::copy(EXE, &exe).unwrap();
    std::fs::write(bin.join("items.txt"), "Something else // 別の項目 [other]\n").unwrap();
    let d = tmp.path().join("r");
    std::fs::create_dir_all(&d).unwrap();
    let stdout = run_exe(&exe, &d, &format!("send {P} 0.3\nwait {WAIT}\nmeasure\nquit\n"), "items-dev\n\n", 19114, 19115, "en");
    let rows = read_rows(&find(&d.join("out"), ".jsonl").unwrap());
    assert_eq!(rows[1]["id"], "eyes_close", "{}", rows[1]);
    assert_eq!(rows[1]["item"], "Close both eyes tightly");
    assert!(!stdout.contains("Something else"), "{stdout}");
    assert!(stdout.contains("Items: 21"), "{stdout}");
}

#[test]
fn e2e_removed_options_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [vec!["--items", "items.txt"], vec!["--move-threshold", "0.05"]] {
        let out = Command::new(EXE)
            .args(&args)
            .arg("--script").arg(tmp.path().join("none.txt"))
            .arg("--out").arg(tmp.path().join("out"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?} が通った");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("unexpected argument") && err.contains(args[0]), "{args:?}: {err}");
        assert!(!tmp.path().join("out").exists(), "{args:?} で書き出し先ができた");
    }
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
fn e2e_english_display_uses_english_names() {
    // 英語の表示では、chatbox も英語の名前になる。ファイルはどちらの表示でも英語
    let tmp = tempfile::tempdir().unwrap();
    let chat = chatbox_listener(19113);
    let d = tmp.path().join("r");
    std::fs::create_dir_all(&d).unwrap();
    run_lang(&d, &full_script().0, "lang-dev\n\n", 19112, 19113, "en");
    std::thread::sleep(Duration::from_millis(200));
    let msgs = chat.lock().unwrap().clone();
    let has = |t: &str| msgs.iter().any(|x| x.contains(t));
    assert!(has("Close both eyes tightly 1/3 (1/21)"), "{msgs:?}");
    assert!(has("Measure 2 more times before going to the next item"), "{msgs:?}");
    assert!(has("Wink: Close the left eye only 1/3 (14/21)"), "{msgs:?}");
    assert!(has("Read aloud with a neutral face Measure to start recording (19/21) e.g. The North Wind"), "{msgs:?}");
    let rows = read_rows(&find(&d.join("out"), ".jsonl").unwrap());
    assert_eq!(rows[1]["item"], "Close both eyes tightly");
    assert_eq!(rows[1]["id"], "eyes_close");
    assert!(rows.iter().filter(|r| r["id"] == "gaze_up").all(|r| r["direction"] == "Up" && r["item"] == "Gaze (face forward): Up"));
}
