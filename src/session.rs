//! 項目を進める状態の機械。ファイル・音・時計・socket に触れず、
//! 時刻・キー・値の写しを受けて、出来事を返す。

use crate::i18n::{lang, tr, Lang, Text};
use crate::items::{Item, ItemKind};
use chrono::{DateTime, Local};
use std::collections::BTreeMap;

/// キーの名前 → 値。
pub type Snapshot = BTreeMap<String, f32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Measure,
    Next,
    Undo,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub reps: usize,
    pub move_threshold: f64,
}

/// 1 回の測定。
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    pub time: DateTime<Local>,
    pub values: Snapshot,
    /// 直前の拾いの列での、キーごとの最大 − 最小
    pub spread: BTreeMap<String, f64>,
    pub moving: bool,
    /// 順に作る表情の向きの番号（それ以外は 0）
    pub dir: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// 区間の始めからの秒
    pub t: f64,
    pub time: DateTime<Local>,
    pub values: Snapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Pose(Vec<Rec>),
    Sequence(Vec<Rec>),
    Interval { samples: Vec<Sample>, recording: bool, start: Option<f64>, duration: Option<f64> },
}

/// 状態の機械が返す出来事。
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// 1 回を記録した。rep は 1 から、of は決まりの回数
    Recorded { label: String, rep: usize, of: usize, moving: bool, keys: usize },
    /// 記録しなかった、または何もしなかった
    Warn(String),
    Undone { label: String },
    IntervalStarted { restarted: bool },
    IntervalStopped { samples: usize, duration: f64 },
    IntervalDiscarded,
    Advanced,
    Finished,
    /// 記録が変わった（書き出しを書き直す）
    Changed,
}

pub struct Session {
    pub items: Vec<Item>,
    pub records: Vec<Record>,
    pub idx: usize,
    pub cfg: Config,
    pub finished: bool,
}

/// 直前の拾いの列（と押した時点の写し）から、キーごとの最大 − 最小を出す。
pub fn spread(recent: &[Snapshot], now: &Snapshot) -> BTreeMap<String, f64> {
    let mut lo: BTreeMap<String, f32> = BTreeMap::new();
    let mut hi: BTreeMap<String, f32> = BTreeMap::new();
    for s in recent.iter().chain(std::iter::once(now)) {
        for (k, v) in s {
            let l = lo.entry(k.clone()).or_insert(*v);
            if *v < *l {
                *l = *v;
            }
            let h = hi.entry(k.clone()).or_insert(*v);
            if *v > *h {
                *h = *v;
            }
        }
    }
    lo.into_iter().map(|(k, l)| { let h = hi[&k]; (k, (h - l) as f64) }).collect()
}

impl Session {
    pub fn new(items: Vec<Item>, cfg: Config) -> Self {
        let records = items
            .iter()
            .map(|i| match i.kind {
                ItemKind::Pose => Record::Pose(Vec::new()),
                ItemKind::Sequence(_) => Record::Sequence(Vec::new()),
                ItemKind::Interval => Record::Interval { samples: Vec::new(), recording: false, start: None, duration: None },
            })
            .collect();
        Session { items, records, idx: 0, cfg, finished: false }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.records.get(self.idx), Some(Record::Interval { recording: true, .. }))
    }

    fn dirs(&self) -> &[Text] {
        match &self.items[self.idx].kind {
            ItemKind::Sequence(d) => d,
            _ => &[],
        }
    }

    /// 順に作る表情で、次に測る向きの番号と周（1 から）。
    pub fn next_dir(&self) -> Option<(usize, usize)> {
        match &self.records[self.idx] {
            Record::Sequence(recs) => {
                let n = self.dirs().len();
                Some((recs.len() % n, recs.len() / n + 1))
            }
            _ => None,
        }
    }

    /// 区間の拾いを足す（記録中のときだけ）。
    pub fn on_sample(&mut self, t: f64, time: DateTime<Local>, snap: &Snapshot) {
        if let Some(Record::Interval { samples, recording: true, start, .. }) = self.records.get_mut(self.idx) {
            let s0 = *start.get_or_insert(t);
            samples.push(Sample { t: t - s0, time, values: snap.clone() });
        }
    }

    /// キーを 1 つ処理する。`t` は起動からの秒、`snap` は押した時点の値、`recent` は直前の拾いの列。
    pub fn handle_key(&mut self, key: Key, t: f64, time: DateTime<Local>, snap: &Snapshot, recent: &[Snapshot]) -> Vec<Event> {
        if self.finished {
            return vec![];
        }
        match key {
            Key::Measure => self.measure(t, time, snap, recent),
            Key::Undo => self.undo(),
            Key::Next => self.next(t, time),
        }
    }

    fn measure(&mut self, t: f64, time: DateTime<Local>, snap: &Snapshot, recent: &[Snapshot]) -> Vec<Event> {
        let reps = self.cfg.reps;
        let name = self.items[self.idx].name.show().to_string();
        let dirs: Vec<Text> = self.dirs().to_vec();
        let next_dir = self.next_dir();
        match &mut self.records[self.idx] {
            Record::Interval { samples, recording, start, duration } => {
                if *recording {
                    let d = t - start.unwrap_or(t);
                    *recording = false;
                    *duration = Some(d);
                    let n = samples.len();
                    vec![Event::IntervalStopped { samples: n, duration: d }, Event::Changed]
                } else {
                    let restarted = !samples.is_empty();
                    samples.clear();
                    *recording = true;
                    *start = Some(t);
                    *duration = None;
                    let mut ev = vec![Event::IntervalStarted { restarted }];
                    if restarted {
                        ev.push(Event::Changed);
                    }
                    ev
                }
            }
            rec => {
                if snap.is_empty() {
                    return vec![Event::Warn(tr("No FT values have arrived yet. Nothing was recorded", "値がまだ 1 つも届いていません。記録しませんでした").into())];
                }
                let sp = spread(recent, snap);
                let moving = sp.values().any(|d| *d > self.cfg.move_threshold);
                let (label, dir, list) = match rec {
                    Record::Pose(list) => (name.clone(), 0, list),
                    Record::Sequence(list) => {
                        let (d, _) = next_dir.unwrap();
                        (format!("{}: {}", name, dirs[d].show()), d, list)
                    }
                    Record::Interval { .. } => unreachable!(),
                };
                list.push(Rec { time, values: snap.clone(), spread: sp, moving, dir });
                let rep = list.iter().filter(|r| r.dir == dir).count();
                vec![Event::Recorded { label, rep, of: reps, moving, keys: snap.len() }, Event::Changed]
            }
        }
    }

    fn undo(&mut self) -> Vec<Event> {
        let name = self.items[self.idx].name.show().to_string();
        let dirs: Vec<Text> = self.dirs().to_vec();
        match &mut self.records[self.idx] {
            Record::Pose(list) => match list.pop() {
                Some(_) => vec![Event::Undone { label: name }, Event::Changed],
                None => vec![Event::Warn(tr("Nothing to undo", "取り消す記録がありません").into())],
            },
            Record::Sequence(list) => match list.pop() {
                Some(r) => vec![Event::Undone { label: format!("{}: {}", name, dirs[r.dir].show()) }, Event::Changed],
                None => vec![Event::Warn(tr("Nothing to undo", "取り消す記録がありません").into())],
            },
            Record::Interval { samples, recording, start, duration } => {
                if samples.is_empty() && !*recording && duration.is_none() {
                    return vec![Event::Warn(tr("Nothing to undo", "取り消す記録がありません").into())];
                }
                samples.clear();
                *recording = false;
                *start = None;
                *duration = None;
                vec![Event::IntervalDiscarded, Event::Changed]
            }
        }
    }

    fn next(&mut self, t: f64, time: DateTime<Local>) -> Vec<Event> {
        let mut ev = Vec::new();
        if self.is_recording() {
            ev.extend(self.measure(t, time, &Snapshot::new(), &[]));
        }
        if self.idx + 1 >= self.items.len() {
            self.finished = true;
            ev.push(Event::Finished);
        } else {
            self.idx += 1;
            ev.push(Event::Advanced);
        }
        ev
    }

    /// 窓に出す、今の項目と進み具合。
    pub fn progress(&self) -> Progress {
        let total = self.items.len();
        if self.finished {
            return Progress {
                no: total,
                total,
                title: tr("Measurement finished", "測定が終わりました").into(),
                step: String::new(),
                hint: String::new(),
                done: true,
            };
        }
        let item = &self.items[self.idx];
        let name = item.name.show().to_string();
        let reps = self.cfg.reps;
        let next_hint = tr("Press D for the next item", "D キーで次へ").to_string();
        let (title, step, hint) = match &self.records[self.idx] {
            Record::Pose(list) => {
                let next = list.len() + 1;
                if list.len() >= reps {
                    (name, format!("{}/{} {}", list.len(), reps, tr("done", "済み")), next_hint)
                } else if next >= 2 {
                    (name, format!("{next}/{reps}"), tr("Relax your face, then make it again", "顔をリラックスさせてから、もう一度").to_string())
                } else {
                    (name, format!("{next}/{reps}"), String::new())
                }
            }
            Record::Sequence(list) => {
                let dirs = self.dirs();
                let (d, round) = self.next_dir().unwrap();
                if list.len() >= dirs.len() * reps && d == 0 {
                    let step = match lang() {
                        Lang::En => format!("{reps} rounds done"),
                        Lang::Ja => format!("{reps}周 済み"),
                    };
                    (name, step, next_hint)
                } else {
                    (format!("{}: {}", name, dirs[d].show()), format!("{round}/{reps}"), String::new())
                }
            }
            Record::Interval { samples, recording, .. } => {
                if *recording {
                    (name, tr("recording, Measure to stop", "記録中 測定で止める").to_string(), String::new())
                } else if !samples.is_empty() {
                    (name, tr("recorded", "記録済み").to_string(), next_hint)
                } else {
                    (name, tr("Measure to start recording", "測定で始める").to_string(), String::new())
                }
            }
        };
        Progress { no: self.idx + 1, total, title, step, hint, done: false }
    }

    /// chatbox に出す、今の項目と進み具合の 1 行。
    pub fn status(&self) -> String {
        let p = self.progress();
        if p.done {
            return p.title;
        }
        let pos = match lang() {
            Lang::En => format!(" ({}/{})", p.no, p.total),
            Lang::Ja => format!("（{}/{}）", p.no, p.total),
        };
        let hint = if p.hint.is_empty() { String::new() } else { format!(" {}", p.hint) };
        format!("{} {}{}{}", p.title, p.step, pos, hint)
    }
}

/// 今の項目と進み具合を、窓で分けて出せる形にしたもの。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// 何番目の項目か（1 から）
    pub no: usize,
    pub total: usize,
    /// 項目の名前（順に作る表情なら向きまで）
    pub title: String,
    /// 回や、区間の記録の状態
    pub step: String,
    /// 次にすること
    pub hint: String,
    pub done: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        vec![
            Item { name: "表A".into(), kind: ItemKind::Pose },
            Item { name: "向き".into(), kind: ItemKind::Sequence(vec!["上".into(), "下".into()]) },
            Item { name: "読み".into(), kind: ItemKind::Interval },
        ]
    }

    fn snap(v: f32) -> Snapshot {
        let mut s = Snapshot::new();
        s.insert("JawOpen".into(), v);
        s
    }

    fn session() -> Session {
        Session::new(items(), Config { reps: 3, move_threshold: 0.05 })
    }

    fn now() -> DateTime<Local> {
        Local::now()
    }

    #[test]
    fn session_pose_records_undo_and_extra() {
        let mut s = session();
        assert_eq!(s.status(), "表A 1/3 (1/3)");
        for v in [0.1, 0.5, 0.2] {
            s.handle_key(Key::Measure, 0.0, now(), &snap(v), &[snap(v)]);
        }
        assert_eq!(s.status(), "表A 3/3 done (1/3) Press D for the next item");
        let ev = s.handle_key(Key::Undo, 0.0, now(), &snap(0.0), &[]);
        assert_eq!(ev[0], Event::Undone { label: "表A".into() });
        assert_eq!(s.status(), "表A 3/3 (1/3) Relax your face, then make it again");
        s.handle_key(Key::Measure, 0.0, now(), &snap(0.3), &[]);
        let ev = s.handle_key(Key::Measure, 0.0, now(), &snap(0.4), &[]);
        assert!(matches!(ev[0], Event::Recorded { rep: 4, of: 3, .. }));
        match &s.records[0] {
            Record::Pose(l) => assert_eq!(l.iter().map(|r| r.values["JawOpen"]).collect::<Vec<_>>(), vec![0.1, 0.5, 0.3, 0.4]),
            _ => panic!(),
        }
    }

    #[test]
    fn session_empty_snapshot_warns_and_undo_nothing_warns() {
        let mut s = session();
        let ev = s.handle_key(Key::Measure, 0.0, now(), &Snapshot::new(), &[]);
        assert!(matches!(ev[0], Event::Warn(_)));
        let ev = s.handle_key(Key::Undo, 0.0, now(), &Snapshot::new(), &[]);
        assert!(matches!(ev[0], Event::Warn(_)));
        assert_eq!(s.records[0], Record::Pose(vec![]));
    }

    #[test]
    fn session_moving_threshold() {
        let mut s = session();
        let ev = s.handle_key(Key::Measure, 0.0, now(), &snap(0.56), &[snap(0.5)]);
        assert!(matches!(ev[0], Event::Recorded { moving: true, .. }));
        let ev = s.handle_key(Key::Measure, 0.0, now(), &snap(0.54), &[snap(0.5)]);
        assert!(matches!(ev[0], Event::Recorded { moving: false, .. }));
    }

    #[test]
    fn session_sequence_rotates_and_undo_returns_direction() {
        let mut s = session();
        s.handle_key(Key::Next, 0.0, now(), &snap(0.0), &[]);
        assert_eq!(s.status(), "向き: 上 1/3 (2/3)");
        s.handle_key(Key::Measure, 0.0, now(), &snap(0.9), &[]);
        assert_eq!(s.status(), "向き: 下 1/3 (2/3)");
        s.handle_key(Key::Measure, 0.0, now(), &snap(0.1), &[]);
        s.handle_key(Key::Measure, 0.0, now(), &snap(0.0), &[]);
        let ev = s.handle_key(Key::Undo, 0.0, now(), &snap(0.0), &[]);
        assert_eq!(ev[0], Event::Undone { label: "向き: 上".into() });
        assert_eq!(s.status(), "向き: 上 2/3 (2/3)");
        for v in [0.8, 0.2, 0.7, 0.3] {
            s.handle_key(Key::Measure, 0.0, now(), &snap(v), &[]);
        }
        assert_eq!(s.status(), "向き 3 rounds done (2/3) Press D for the next item");
        let ev = s.handle_key(Key::Measure, 0.0, now(), &snap(0.6), &[]);
        assert!(matches!(&ev[0], Event::Recorded { label, rep: 4, .. } if label == "向き: 上"));
        match &s.records[1] {
            Record::Sequence(l) => {
                let up: Vec<f32> = l.iter().filter(|r| r.dir == 0).map(|r| r.values["JawOpen"]).collect();
                let down: Vec<f32> = l.iter().filter(|r| r.dir == 1).map(|r| r.values["JawOpen"]).collect();
                assert_eq!(up, vec![0.9, 0.8, 0.7, 0.6]);
                assert_eq!(down, vec![0.1, 0.2, 0.3]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn session_interval_start_stop_restart_undo_and_next_while_recording() {
        let mut s = session();
        s.idx = 2;
        s.on_sample(0.5, now(), &snap(9.0)); // 記録していない間は入らない
        s.handle_key(Key::Measure, 1.0, now(), &snap(0.0), &[]);
        assert!(s.is_recording());
        s.on_sample(1.0, now(), &snap(0.2));
        s.on_sample(1.05, now(), &snap(0.4));
        let ev = s.handle_key(Key::Measure, 1.5, now(), &snap(0.0), &[]);
        assert_eq!(ev[0], Event::IntervalStopped { samples: 2, duration: 0.5 });
        s.on_sample(1.6, now(), &snap(9.0));
        match &s.records[2] {
            Record::Interval { samples, duration, .. } => {
                assert_eq!(samples.iter().map(|x| x.values["JawOpen"]).collect::<Vec<_>>(), vec![0.2, 0.4]);
                assert_eq!(*duration, Some(0.5));
            }
            _ => panic!(),
        }
        let ev = s.handle_key(Key::Measure, 2.0, now(), &snap(0.0), &[]);
        assert_eq!(ev[0], Event::IntervalStarted { restarted: true });
        let ev = s.handle_key(Key::Undo, 2.0, now(), &snap(0.0), &[]);
        assert_eq!(ev[0], Event::IntervalDiscarded);
        assert!(!s.is_recording());
        s.handle_key(Key::Measure, 3.0, now(), &snap(0.0), &[]);
        s.on_sample(3.0, now(), &snap(0.7));
        let ev = s.handle_key(Key::Next, 3.2, now(), &snap(0.0), &[]);
        assert!(matches!(ev[0], Event::IntervalStopped { samples: 1, .. }));
        assert_eq!(*ev.last().unwrap(), Event::Finished);
        assert!(s.finished);
    }
}
