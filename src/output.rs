//! 測定の記録を JSON Lines の 1 つのファイルに書く。記録が変わるたびに全部を書き直す。

use crate::items::ItemKind;
use crate::session::{Record, Session};
use crate::stats::round6;
use chrono::{DateTime, Local};
use serde_json::{json, Map, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 書き出す形の版。生成器が読む形を変えたとき（キーの意味を変える・消す、項目の id を変える・消す）に上げる。
/// キーや項目を足すだけなら上げない。`format_version` を持たない前の出力（`_raw.jsonl`）を版 0 とみなす
pub const FORMAT_VERSION: u32 = 1;

pub struct SessionInfo {
    pub device: String,
    pub memo: String,
    pub started: DateTime<Local>,
    pub prefix: String,
    pub tool_version: String,
}

pub struct Writer {
    pub path: PathBuf,
    pub info: SessionInfo,
}

/// ファイルの名前に使えない文字を `_` にする。
pub fn safe_name(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let t = t.trim().to_string();
    if t.is_empty() { "unnamed".into() } else { t }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

fn values_json(v: &std::collections::BTreeMap<String, f32>) -> Value {
    let mut m = Map::new();
    for (k, x) in v {
        m.insert(k.clone(), json!(round6(*x as f64)));
    }
    Value::Object(m)
}

impl Writer {
    pub fn new(dir: &Path, info: SessionInfo) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let base = format!("{}_{}", safe_name(&info.device), info.started.format("%Y%m%d-%H%M%S"));
        Ok(Writer { path: dir.join(format!("{base}.jsonl")), info })
    }

    pub fn jsonl(&self, session: &Session) -> String {
        let mut lines = vec![json!({
            "type": "session",
            "format_version": FORMAT_VERSION,
            "device": self.info.device,
            "memo": self.info.memo,
            "started": self.info.started.to_rfc3339(),
            "prefix": self.info.prefix,
            "tool_version": self.info.tool_version,
        })];
        for (i, (item, rec)) in session.items.iter().zip(&session.records).enumerate() {
            let no = i + 1;
            match (rec, &item.kind) {
                (Record::Pose(list), _) => {
                    for (j, r) in list.iter().enumerate() {
                        lines.push(json!({
                            "type": "pose", "item_no": no, "item": item.name.en, "id": item.id(0), "rep": j + 1,
                            "time": r.time.to_rfc3339(),
                            "values": values_json(&r.values),
                            "spread": r.spread.iter().map(|(k, v)| (k.clone(), json!(round6(*v)))).collect::<Map<_, _>>(),
                        }));
                    }
                }
                (Record::Sequence(list), ItemKind::Sequence(dirs)) => {
                    let mut counts = vec![0usize; dirs.len()];
                    for r in list {
                        counts[r.dir] += 1;
                        lines.push(json!({
                            "type": "pose", "item_no": no, "item": format!("{}: {}", item.name.en, dirs[r.dir].en),
                            "id": item.id(r.dir), "direction": dirs[r.dir].en, "rep": counts[r.dir],
                            "time": r.time.to_rfc3339(),
                            "values": values_json(&r.values),
                            "spread": r.spread.iter().map(|(k, v)| (k.clone(), json!(round6(*v)))).collect::<Map<_, _>>(),
                        }));
                    }
                }
                (Record::Interval { samples, recording: false, duration: Some(_), .. }, _) => {
                    for (j, s) in samples.iter().enumerate() {
                        lines.push(json!({
                            "type": "sample", "item_no": no, "item": item.name.en, "id": item.id(0), "n": j + 1,
                            "t": round6(s.t), "time": s.time.to_rfc3339(),
                            "values": values_json(&s.values),
                        }));
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for l in lines {
            text.push_str(&l.to_string());
            text.push('\n');
        }
        text
    }

    pub fn write_all(&self, session: &Session) -> io::Result<()> {
        write_atomic(&self.path, self.jsonl(session).as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::Item;
    use crate::session::{Config, Key, Snapshot};

    fn snap(pairs: &[(&str, f32)]) -> Snapshot {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn output_writes_one_jsonl_with_version_and_ids() {
        let dir = tempfile::tempdir().unwrap();
        let info = SessionInfo {
            device: "vi:ve/1".into(), memo: "照明あり, カメラ上".into(), started: Local::now(),
            prefix: "FT/In/".into(), tool_version: "0.2.0".into(),
        };
        let w = Writer::new(dir.path(), info).unwrap();
        let name = w.path.file_name().unwrap().to_str().unwrap().to_string();
        assert!(name.starts_with("vi_ve_1_") && name.ends_with(".jsonl") && !name.contains("_raw"), "{name}");
        let items = vec![
            Item { name: "表A".into(), kind: ItemKind::Pose, ids: vec!["a".into()] },
            Item { name: "向き".into(), kind: ItemKind::Sequence(vec!["上".into(), "下".into()]), ids: vec!["up".into(), "down".into()] },
            Item { name: "読み".into(), kind: ItemKind::Interval(None), ids: vec!["read".into()] },
        ];
        let mut s = Session::new(items, Config { reps: 3, move_threshold: 0.05 });
        let t = Local::now();
        for v in [0.1, 0.5, 0.2] {
            s.handle_key(Key::Measure, 0.0, t, &snap(&[("JawOpen", v), ("Smile", 0.000002)]), &[]);
        }
        s.handle_key(Key::Measure, 0.0, t, &snap(&[("JawOpen", 0.9)]), &[]);
        s.handle_key(Key::Undo, 0.0, t, &Snapshot::new(), &[]);
        s.handle_key(Key::Next, 0.0, t, &Snapshot::new(), &[]);
        for v in [0.9, 0.1, 0.8, 0.2, 0.7, 0.3] {
            s.handle_key(Key::Measure, 0.0, t, &snap(&[("JawOpen", v)]), &[]);
        }
        s.handle_key(Key::Next, 0.0, t, &Snapshot::new(), &[]);
        s.handle_key(Key::Measure, 10.0, t, &Snapshot::new(), &[]);
        for (i, v) in [1.0f32, 2.0, 3.0, 4.0].iter().enumerate() {
            s.on_sample(10.0 + i as f64 * 0.05, t, &snap(&[("JawOpen", *v)]));
        }
        s.handle_key(Key::Measure, 10.5, t, &Snapshot::new(), &[]);
        w.write_all(&s).unwrap();

        // 書き出すのは 1 つのファイルだけ
        let files: Vec<String> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(files, vec![name.clone()]);

        let raw = fs::read_to_string(&w.path).unwrap();
        let rows: Vec<Value> = raw.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows[0]["type"], "session");
        assert_eq!(rows[0]["format_version"], 1);
        assert_eq!(rows[0]["device"], "vi:ve/1");
        assert_eq!(rows[0]["memo"], "照明あり, カメラ上");
        assert_eq!(rows[0]["prefix"], "FT/In/");
        assert_eq!(rows[0]["tool_version"], "0.2.0");
        assert!(rows[0].get("items_file").is_none());
        assert!(rows[0]["started"].as_str().unwrap().len() > 10);
        // 取り消した回は残らない
        let a: Vec<f64> = rows.iter().filter(|r| r["id"] == "a").map(|r| r["values"]["JawOpen"].as_f64().unwrap()).collect();
        assert_eq!(a, vec![0.1, 0.5, 0.2]);
        // 順に作る表情は向きごとの id を持つ
        let up: Vec<f64> = rows.iter().filter(|r| r["id"] == "up").map(|r| r["values"]["JawOpen"].as_f64().unwrap()).collect();
        let down: Vec<f64> = rows.iter().filter(|r| r["id"] == "down").map(|r| r["values"]["JawOpen"].as_f64().unwrap()).collect();
        assert_eq!((up, down), (vec![0.9, 0.8, 0.7], vec![0.1, 0.2, 0.3]));
        assert!(rows.iter().filter(|r| r["id"] == "up").all(|r| r["item"] == "向き: 上" && r["direction"] == "上"));
        assert_eq!(rows.iter().filter(|r| r["type"] == "sample" && r["id"] == "read").count(), 4);
        assert!(rows.iter().filter(|r| r["type"] == "sample").all(|r| r["n"].as_u64().is_some()));
        // 2 行目からの全部の行が id を持ち、moving は無く、pose の行は spread を持つ
        for r in &rows[1..] {
            assert!(r["id"].as_str().is_some_and(|x| !x.is_empty()), "{r}");
            assert!(r.get("moving").is_none(), "{r}");
            if r["type"] == "pose" {
                assert!(r["spread"].is_object(), "{r}");
            }
        }
    }
}
