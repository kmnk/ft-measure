//! exe に埋め込んだ項目の一覧を読む。

use crate::i18n::{tr, Text};

/// 項目の一覧（exe に埋め込む。利用者は変えられない）。
pub const DEFAULT_ITEMS: &str = include_str!("items.txt");

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// 普通の顔に戻しながら決まった回数を測る表情
    Pose,
    /// 向きを順に回って測る表情。向きの名前の列
    Sequence(Vec<Text>),
    /// 測定で始めて測定で止める区間。読み上げる文の例があれば持つ
    Interval(Option<Text>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: Text,
    pub kind: ItemKind,
    /// 書き出す行の id。3 回測る項目と区間は 1 つ、順に作る表情は向きの数だけ
    pub ids: Vec<String>,
}

impl Item {
    /// `dir` 番目の向き（順に作る表情でなければ 0）の id。
    pub fn id(&self, dir: usize) -> &str {
        self.ids.get(dir).or(self.ids.first()).map(String::as_str).unwrap_or("")
    }
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 名前の末尾の ` [id]` を外し、名前と id に分ける。
fn split_id(s: &str) -> (&str, Option<&str>) {
    let t = s.trim_end();
    if let Some(open) = t.rfind('[') {
        if t.ends_with(']') {
            return (t[..open].trim_end(), Some(&t[open + 1..t.len() - 1]));
        }
    }
    (t, None)
}

fn strip_tag<'a>(line: &'a str, tags: &[&str]) -> Option<&'a str> {
    for tag in tags {
        for colon in [":", "："] {
            let t = format!("{tag}{colon}");
            if line.len() >= t.len() && line.is_char_boundary(t.len()) && line[..t.len()].eq_ignore_ascii_case(&t) {
                return Some(line[t.len()..].trim());
            }
        }
    }
    None
}

/// `English // 日本語` を 2 つの名前にする。`//` が無ければ、どちらも同じ名前にする。
fn text(s: &str) -> Text {
    match s.split_once("//") {
        Some((en, ja)) if !en.trim().is_empty() && !ja.trim().is_empty() => Text::new(en.trim(), ja.trim()),
        Some((en, ja)) => Text::same(if en.trim().is_empty() { ja.trim() } else { en.trim() }),
        None => Text::same(s.trim()),
    }
}

/// 一覧の文字列を読む。誤りは「何行目が何か」で返す。
pub fn parse(src: &str) -> Result<Vec<Item>, String> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut items = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = i + 1;
        let err = |m: String| format!("{} {lineno}: {m}", tr("line", "行"));
        // 名前の部分から id を取り出す。無い・形が違う・前に出た id は誤り
        let mut take_id = |part: &str| -> Result<(String, String), String> {
            let (name, id) = split_id(part);
            let id = id.ok_or_else(|| err(format!("{}: {}", tr("no [id]", "[id] がありません"), name.trim())))?;
            if !is_id(id) {
                return Err(err(format!("{}: {id}", tr("an id has only a-z, 0-9 and _", "id に使えるのは a-z・0-9・_ だけです"))));
            }
            if seen.iter().any(|s| s == id) {
                return Err(err(format!("{}: {id}", tr("the id is used twice", "id が重なっています"))));
            }
            seen.push(id.to_string());
            Ok((name.to_string(), id.to_string()))
        };
        let item = if let Some(rest) = strip_tag(line, &["interval", "区間"]) {
            if rest.is_empty() {
                return Err(format!("{} {lineno}: {}", tr("line", "行"), tr("the interval item has no name", "区間の項目の名前がありません")));
            }
            // `名前 | 読み上げる文の例` の形。例は無くてもよい
            let (name, read) = match rest.split_once(['|', '｜']) {
                Some((n, r)) if !r.trim().is_empty() => (n, Some(text(r))),
                Some((n, _)) => (n, None),
                None => (rest, None),
            };
            if split_id(name).0.trim().is_empty() {
                return Err(format!("{} {lineno}: {}", tr("line", "行"), tr("the interval item has no name", "区間の項目の名前がありません")));
            }
            let (name, id) = take_id(name)?;
            Item { name: text(&name), kind: ItemKind::Interval(read), ids: vec![id] }
        } else if let Some(rest) = strip_tag(line, &["sequence", "順に"]) {
            let raw_parts: Vec<&str> = rest.split(['|', '｜']).filter(|s| !s.trim().is_empty()).collect();
            if raw_parts.len() < 2 {
                return Err(format!(
                    "{} {lineno}: {}",
                    tr("line", "行"),
                    tr("write a sequence as \"name | step | step…\" with at least one step", "順に作る表情は「名前 | 向き | 向き…」の形で、向きが 1 つ以上要ります")
                ));
            }
            if split_id(raw_parts[0]).1.is_some() {
                return Err(err(tr("write the ids on the steps, not on the sequence name", "id は順に作る表情の名前ではなく、各向きに書きます").to_string()));
            }
            let mut steps = Vec::new();
            let mut ids = Vec::new();
            for p in &raw_parts[1..] {
                let (name, id) = take_id(p)?;
                steps.push(text(&name));
                ids.push(id);
            }
            Item { name: text(raw_parts[0]), kind: ItemKind::Sequence(steps), ids }
        } else {
            let (name, id) = take_id(line)?;
            Item { name: text(&name), kind: ItemKind::Pose, ids: vec![id] }
        };
        items.push(item);
    }
    if items.is_empty() {
        return Err(tr("the list has no items", "項目が 1 つもありません").to_string());
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_parse_kinds_and_skip_comments() {
        let t = "\u{feff}# c\n\n両目を閉じる [a]\n順に: 目線 | 上 [b] | 下 [c]\n区間：読み上げ [d]\n  順に：舌｜上 [e]｜下 [f]  \n";
        let v = parse(t).unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(v[0], Item { name: "両目を閉じる".into(), kind: ItemKind::Pose, ids: vec!["a".into()] });
        assert_eq!(v[1].kind, ItemKind::Sequence(vec!["上".into(), "下".into()]));
        assert_eq!(v[1].name, Text::same("目線"));
        assert_eq!(v[1].ids, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(v[2], Item { name: "読み上げ".into(), kind: ItemKind::Interval(None), ids: vec!["d".into()] });
        assert_eq!(v[3].kind, ItemKind::Sequence(vec!["上".into(), "下".into()]));
        assert_eq!((v[3].id(0), v[3].id(1)), ("e", "f"));
        assert_eq!(v[0].id(0), "a");
    }

    #[test]
    fn items_parse_english_tags_and_two_languages() {
        let t = "Close eyes // 目を閉じる [eyes]\nSequence: Gaze // 目線 | Up // 上 [up] | Down [down]\ninterval: Read // 読む [read]\n";
        let v = parse(t).unwrap();
        assert_eq!(v[0].name, Text::new("Close eyes", "目を閉じる"));
        assert_eq!(v[1].name, Text::new("Gaze", "目線"));
        assert_eq!(v[1].kind, ItemKind::Sequence(vec![Text::new("Up", "上"), Text::same("Down")]));
        assert_eq!(v[2], Item { name: Text::new("Read", "読む"), kind: ItemKind::Interval(None), ids: vec!["read".into()] });
    }

    #[test]
    fn items_parse_errors() {
        assert!(parse("# only\n").is_err());
        assert!(parse("順に: 目線\n").unwrap_err().contains(" 1:"));
        assert!(parse("a [a]\n区間:\n").unwrap_err().contains(" 2:"));
    }

    #[test]
    fn items_parse_id_errors() {
        // id が無い（3 回測る項目、向き、区間）
        assert!(parse("a\n").unwrap_err().contains(" 1:"));
        assert!(parse("x [x]\nsequence: s | up [u] | down\n").unwrap_err().contains(" 2:"));
        assert!(parse("interval: r | example\n").unwrap_err().contains(" 1:"));
        // 形が違う
        assert!(parse("a [mouth-smile]\n").unwrap_err().contains(" 1:"));
        assert!(parse("a [Smile]\n").unwrap_err().contains(" 1:"));
        assert!(parse("a []\n").unwrap_err().contains(" 1:"));
        // 重なる（項目どうし、向きどうし）
        assert!(parse("a [x]\nb [y]\nc [x]\n").unwrap_err().contains(" 3:"));
        assert!(parse("sequence: s | up [u] | down [u]\n").unwrap_err().contains(" 1:"));
        // 順に作る表情の名前に付けた
        assert!(parse("sequence: s [s] | up [u] | down [d]\n").unwrap_err().contains(" 1:"));
    }

    #[test]
    fn items_interval_reads_example() {
        let v = parse("interval: Read // 読む [r] | The North Wind // あめんぼ赤いな\n区間: 読む2 [r2] |\n").unwrap();
        assert_eq!(v[0], Item { name: Text::new("Read", "読む"), kind: ItemKind::Interval(Some(Text::new("The North Wind", "あめんぼ赤いな"))), ids: vec!["r".into()] });
        assert_eq!(v[1], Item { name: "読む2".into(), kind: ItemKind::Interval(None), ids: vec!["r2".into()] });
        assert!(parse("interval: | 例\n").unwrap_err().contains(" 1:"));
        assert!(parse("interval: [r] | 例\n").unwrap_err().contains(" 1:"));
    }

    /// spec.md の「項目の `id`」の表。書き出す行の項目の英語の名前と id
    const SPEC_IDS: &[(&str, &str)] = &[
        ("Close both eyes tightly", "eyes_close"),
        ("Open both eyes as wide as you can", "eyes_wide"),
        ("Squint by raising the lower eyelids (as in bright light)", "eyes_squint"),
        ("Raise both brows as high as you can (surprise)", "brows_up"),
        ("Pull the brows together and down (frown)", "brows_down"),
        ("Open the mouth vertically as far as you can", "jaw_open"),
        ("With the mouth open, stretch the lips sideways", "mouth_stretch"),
        ("With the mouth closed, pull the corners out and up", "mouth_smile"),
        ("With the mouth closed, pull the corners down", "mouth_frown"),
        ("Pucker the lips and push them forward", "lips_pucker"),
        ("Bite the teeth together and open the lips (\"eee\")", "lips_apart"),
        ("Close the mouth and puff out the cheeks", "cheeks_puff"),
        ("Stick the tongue out as far as you can", "tongue_out"),
        ("Wink: Close the right eye only", "wink_right"),
        ("Wink: Close the left eye only", "wink_left"),
        ("Gaze (face forward): Up", "gaze_up"),
        ("Gaze (face forward): Down", "gaze_down"),
        ("Gaze (face forward): Right", "gaze_right"),
        ("Gaze (face forward): Left", "gaze_left"),
        ("Lips: Press the lips together hard (\"mmm\")", "lips_press"),
        ("Lips: Roll the lips into the mouth to hide them", "lips_roll"),
        ("Jaw (mouth slightly open): Shift right", "jaw_right"),
        ("Jaw (mouth slightly open): Shift left", "jaw_left"),
        ("Tongue out, point the tip: Up", "tongue_up"),
        ("Tongue out, point the tip: Down", "tongue_down"),
        ("Tongue out, point the tip: Right", "tongue_right"),
        ("Tongue out, point the tip: Left", "tongue_left"),
        ("Read aloud with a neutral face", "read_neutral"),
        ("Read aloud with a light smile", "read_smile"),
        ("Relaxed neutral face", "neutral"),
    ];

    #[test]
    fn items_default_list_matches_spec() {
        let v = parse(DEFAULT_ITEMS).unwrap();
        assert_eq!(v.len(), 21);
        let pose = v.iter().filter(|i| i.kind == ItemKind::Pose).count();
        let seq: Vec<usize> = v
            .iter()
            .filter_map(|i| match &i.kind {
                ItemKind::Sequence(d) => Some(d.len()),
                _ => None,
            })
            .collect();
        let interval = v.iter().filter(|i| matches!(i.kind, ItemKind::Interval(Some(_)))).count();
        // 表情 13 + 無表情 1、順に 5（向きは合わせて 14）、区間 2
        assert_eq!(pose, 14);
        assert_eq!(seq.len(), 5);
        assert_eq!(seq.iter().sum::<usize>(), 14);
        assert_eq!(interval, 2);
        assert_eq!(v.last().unwrap().name.ja, "力を抜いた無表情");
        // 書き出しに使う英語の名前が、どの項目と向きにもある
        for i in &v {
            assert!(i.name.en.is_ascii() && i.name.en != i.name.ja, "{:?}", i.name);
            if let ItemKind::Sequence(d) = &i.kind {
                assert!(d.iter().all(|x| x.en.is_ascii() && x.en != x.ja), "{d:?}");
            }
        }
        // 書き出す行の名前と id の組が spec の表と一致する
        let mut got: Vec<(String, String)> = Vec::new();
        for i in &v {
            match &i.kind {
                ItemKind::Sequence(d) => {
                    for (k, s) in d.iter().enumerate() {
                        got.push((format!("{}: {}", i.name.en, s.en), i.id(k).to_string()));
                    }
                }
                _ => got.push((i.name.en.clone(), i.id(0).to_string())),
            }
        }
        let want: Vec<(String, String)> =
            SPEC_IDS.iter().map(|(n, id)| (n.to_string(), id.to_string())).collect();
        assert_eq!(got, want);
    }
}
