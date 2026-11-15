//! 項目の一覧のファイルを読む。

use crate::i18n::{tr, Text};

/// 既定の一覧（exe に埋め込む）。
pub const DEFAULT_ITEMS: &str = include_str!("../items.txt");

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
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = i + 1;
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
            if name.trim().is_empty() {
                return Err(format!("{} {lineno}: {}", tr("line", "行"), tr("the interval item has no name", "区間の項目の名前がありません")));
            }
            Item { name: text(name), kind: ItemKind::Interval(read) }
        } else if let Some(rest) = strip_tag(line, &["sequence", "順に"]) {
            let parts: Vec<Text> = rest.split(['|', '｜']).filter(|s| !s.trim().is_empty()).map(text).collect();
            if parts.len() < 2 {
                return Err(format!(
                    "{} {lineno}: {}",
                    tr("line", "行"),
                    tr("write a sequence as \"name | step | step…\" with at least one step", "順に作る表情は「名前 | 向き | 向き…」の形で、向きが 1 つ以上要ります")
                ));
            }
            Item { name: parts[0].clone(), kind: ItemKind::Sequence(parts[1..].to_vec()) }
        } else {
            Item { name: text(line), kind: ItemKind::Pose }
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
        let t = "\u{feff}# c\n\n両目を閉じる\n順に: 目線 | 上 | 下\n区間：読み上げ\n  順に：舌｜上｜下  \n";
        let v = parse(t).unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(v[0], Item { name: "両目を閉じる".into(), kind: ItemKind::Pose });
        assert_eq!(v[1].kind, ItemKind::Sequence(vec!["上".into(), "下".into()]));
        assert_eq!(v[1].name, Text::same("目線"));
        assert_eq!(v[2], Item { name: "読み上げ".into(), kind: ItemKind::Interval(None) });
        assert_eq!(v[3].kind, ItemKind::Sequence(vec!["上".into(), "下".into()]));
    }

    #[test]
    fn items_parse_english_tags_and_two_languages() {
        let t = "Close eyes // 目を閉じる\nSequence: Gaze // 目線 | Up // 上 | Down\ninterval: Read // 読む\n";
        let v = parse(t).unwrap();
        assert_eq!(v[0].name, Text::new("Close eyes", "目を閉じる"));
        assert_eq!(v[1].name, Text::new("Gaze", "目線"));
        assert_eq!(v[1].kind, ItemKind::Sequence(vec![Text::new("Up", "上"), Text::same("Down")]));
        assert_eq!(v[2], Item { name: Text::new("Read", "読む"), kind: ItemKind::Interval(None) });
    }

    #[test]
    fn items_parse_errors() {
        assert!(parse("# only\n").is_err());
        assert!(parse("順に: 目線\n").unwrap_err().contains(" 1:"));
        assert!(parse("a\n区間:\n").unwrap_err().contains(" 2:"));
    }

    #[test]
    fn items_interval_reads_example() {
        let v = parse("interval: Read // 読む | The North Wind // あめんぼ赤いな\n区間: 読む2 |\n").unwrap();
        assert_eq!(v[0], Item { name: Text::new("Read", "読む"), kind: ItemKind::Interval(Some(Text::new("The North Wind", "あめんぼ赤いな"))) });
        assert_eq!(v[1], Item { name: "読む2".into(), kind: ItemKind::Interval(None) });
        assert!(parse("interval: | 例\n").unwrap_err().contains(" 1:"));
    }

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
    }
}
