//! summary.csv と raw.jsonl を書く。記録が変わるたびに全部を書き直す。

use crate::items::ItemKind;
use crate::session::{Rec, Record, Session};
use crate::stats::{fmt6, round6, summarize};
use chrono::{DateTime, Local};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct SessionInfo {
    pub device: String,
    pub memo: String,
    pub started: DateTime<Local>,
    pub prefix: String,
    pub tool_version: String,
}

pub struct Writer {
    pub summary_path: PathBuf,
    pub raw_path: PathBuf,
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

const COLUMNS: [&str; 14] = [
    "device", "memo", "item_no", "item", "kind", "key", "n", "duration_s", "median", "min", "max", "p5", "p95", "moving_n",
];

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
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
        Ok(Writer {
            summary_path: dir.join(format!("{base}_summary.csv")),
            raw_path: dir.join(format!("{base}_raw.jsonl")),
            info,
        })
    }

    fn pose_rows(&self, out: &mut Vec<Vec<String>>, item_no: usize, label: &str, recs: &[&Rec]) {
        let keys: BTreeSet<&String> = recs.iter().flat_map(|r| r.values.keys()).collect();
        let moving_n = recs.iter().filter(|r| r.moving).count();
        for k in keys {
            let vals: Vec<f64> = recs.iter().filter_map(|r| r.values.get(k)).map(|v| *v as f64).collect();
            let Some(s) = summarize(&vals) else { continue };
            out.push(vec![
                self.info.device.clone(), self.info.memo.clone(), item_no.to_string(), label.to_string(), "pose".into(),
                k.clone(), s.n.to_string(), String::new(), fmt6(s.median), fmt6(s.min), fmt6(s.max), String::new(),
                String::new(), moving_n.to_string(),
            ]);
        }
    }

    pub fn summary_csv(&self, session: &Session) -> String {
        let mut rows: Vec<Vec<String>> = Vec::new();
        for (i, (item, rec)) in session.items.iter().zip(&session.records).enumerate() {
            let no = i + 1;
            match (rec, &item.kind) {
                (Record::Pose(list), _) => {
                    let refs: Vec<&Rec> = list.iter().collect();
                    self.pose_rows(&mut rows, no, &item.name.en, &refs);
                }
                (Record::Sequence(list), ItemKind::Sequence(dirs)) => {
                    for (d, dir) in dirs.iter().enumerate() {
                        let refs: Vec<&Rec> = list.iter().filter(|r| r.dir == d).collect();
                        self.pose_rows(&mut rows, no, &format!("{}: {}", item.name.en, dir.en), &refs);
                    }
                }
                (Record::Interval { samples, recording: false, duration: Some(dur), .. }, _) => {
                    let keys: BTreeSet<&String> = samples.iter().flat_map(|s| s.values.keys()).collect();
                    for k in keys {
                        let vals: Vec<f64> = samples.iter().filter_map(|s| s.values.get(k)).map(|v| *v as f64).collect();
                        let Some(s) = summarize(&vals) else { continue };
                        rows.push(vec![
                            self.info.device.clone(), self.info.memo.clone(), no.to_string(), item.name.en.clone(),
                            "interval".into(), k.clone(), s.n.to_string(), fmt6(*dur), fmt6(s.median), fmt6(s.min),
                            fmt6(s.max), fmt6(s.p5), fmt6(s.p95), String::new(),
                        ]);
                    }
                }
                _ => {}
            }
        }
        let mut text = String::from("\u{feff}");
        text.push_str(&COLUMNS.join(","));
        text.push_str("\r\n");
        for r in rows {
            text.push_str(&r.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
            text.push_str("\r\n");
        }
        text
    }

    pub fn raw_jsonl(&self, session: &Session) -> String {
        let mut lines = vec![json!({
            "type": "session",
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
                            "type": "pose", "item_no": no, "item": item.name.en, "rep": j + 1,
                            "time": r.time.to_rfc3339(), "moving": r.moving,
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
                            "direction": dirs[r.dir].en, "rep": counts[r.dir],
                            "time": r.time.to_rfc3339(), "moving": r.moving,
                            "values": values_json(&r.values),
                            "spread": r.spread.iter().map(|(k, v)| (k.clone(), json!(round6(*v)))).collect::<Map<_, _>>(),
                        }));
                    }
                }
                (Record::Interval { samples, recording: false, duration: Some(_), .. }, _) => {
                    for (j, s) in samples.iter().enumerate() {
                        lines.push(json!({
                            "type": "sample", "item_no": no, "item": item.name.en, "n": j + 1,
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
        write_atomic(&self.summary_path, self.summary_csv(session).as_bytes())?;
        write_atomic(&self.raw_path, self.raw_jsonl(session).as_bytes())
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
    fn output_writes_summary_and_raw() {
        let dir = tempfile::tempdir().unwrap();
        let info = SessionInfo {
            device: "vi:ve/1".into(), memo: "照明あり, カメラ上".into(), started: Local::now(),
            prefix: "FT/In/".into(), tool_version: "0.1.0".into(),
        };
        let w = Writer::new(dir.path(), info).unwrap();
        assert!(w.summary_path.file_name().unwrap().to_str().unwrap().starts_with("vi_ve_1_"));
        let items = vec![
            Item { name: "表A".into(), kind: ItemKind::Pose, ids: vec!["a".into()] },
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
        s.handle_key(Key::Measure, 10.0, t, &Snapshot::new(), &[]);
        for (i, v) in [1.0f32, 2.0, 3.0, 4.0].iter().enumerate() {
            s.on_sample(10.0 + i as f64 * 0.05, t, &snap(&[("JawOpen", *v)]));
        }
        s.handle_key(Key::Measure, 10.5, t, &Snapshot::new(), &[]);
        w.write_all(&s).unwrap();

        let csv = fs::read_to_string(&w.summary_path).unwrap();
        assert!(csv.starts_with('\u{feff}'));
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines[0], COLUMNS.join(","));
        assert_eq!(lines[0].split(',').count(), 14);
        assert!(lines.contains(&"vi:ve/1,\"照明あり, カメラ上\",1,表A,pose,JawOpen,3,,0.2,0.1,0.5,,,0"), "{csv}");
        assert!(lines.contains(&"vi:ve/1,\"照明あり, カメラ上\",1,表A,pose,Smile,3,,0.000002,0.000002,0.000002,,,0"), "{csv}");
        assert!(lines.contains(&"vi:ve/1,\"照明あり, カメラ上\",2,読み,interval,JawOpen,4,0.5,2.5,1,4,1.15,3.85,"), "{csv}");
        assert!(!csv.contains("0.9"), "取り消した回が残っている: {csv}");
        assert!(!csv.contains("e-") && !csv.contains("E-"), "指数の表記がある: {csv}");

        let raw = fs::read_to_string(&w.raw_path).unwrap();
        let rows: Vec<Value> = raw.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows[0]["type"], "session");
        assert_eq!(rows[0]["device"], "vi:ve/1");
        assert_eq!(rows[0]["memo"], "照明あり, カメラ上");
        assert!(rows[0].get("items_file").is_none());
        assert_eq!(rows[0]["prefix"], "FT/In/");
        assert_eq!(rows[0]["tool_version"], "0.1.0");
        assert!(rows[0]["started"].as_str().unwrap().len() > 10);
        assert_eq!(rows.iter().filter(|r| r["type"] == "pose").count(), 3);
        assert_eq!(rows.iter().filter(|r| r["type"] == "sample").count(), 4);
        assert!(rows.iter().filter(|r| r["type"] == "sample").all(|r| r["n"].as_u64().is_some()));
    }
}
