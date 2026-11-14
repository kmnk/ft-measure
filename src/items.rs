//! 項目の一覧のファイルを読む。

/// 既定の一覧（exe に埋め込む）。
pub const DEFAULT_ITEMS: &str = include_str!("../items.txt");

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// 普通の顔に戻しながら決まった回数を測る表情
    Pose,
    /// 向きを順に回って測る表情。向きの名前の列
    Sequence(Vec<String>),
    /// 測定で始めて測定で止める区間
    Interval,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub kind: ItemKind,
}

fn strip_tag<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
    for colon in [":", "："] {
        let t = format!("{tag}{colon}");
        if let Some(rest) = line.strip_prefix(&t) {
            return Some(rest.trim());
        }
    }
    None
}

/// 一覧の文字列を読む。誤りは「何行目が何か」で返す。
pub fn parse(text: &str) -> Result<Vec<Item>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut items = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = i + 1;
        let item = if let Some(rest) = strip_tag(line, "区間") {
            if rest.is_empty() {
                return Err(format!("{lineno} 行目: 区間の項目の名前がありません"));
            }
            Item { name: rest.to_string(), kind: ItemKind::Interval }
        } else if let Some(rest) = strip_tag(line, "順に") {
            let parts: Vec<String> = rest
                .split(['|', '｜'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if parts.len() < 2 {
                return Err(format!(
                    "{lineno} 行目: 順に作る表情は「名前 | 向き | 向き…」の形で、向きが 1 つ以上要ります"
                ));
            }
            Item { name: parts[0].clone(), kind: ItemKind::Sequence(parts[1..].to_vec()) }
        } else {
            Item { name: line.to_string(), kind: ItemKind::Pose }
        };
        items.push(item);
    }
    if items.is_empty() {
        return Err("項目が 1 つもありません".to_string());
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
        assert_eq!(v[1].name, "目線");
        assert_eq!(v[2], Item { name: "読み上げ".into(), kind: ItemKind::Interval });
        assert_eq!(v[3].kind, ItemKind::Sequence(vec!["上".into(), "下".into()]));
    }

    #[test]
    fn items_parse_errors() {
        assert!(parse("# only\n").is_err());
        assert!(parse("順に: 目線\n").unwrap_err().contains("1 行目"));
        assert!(parse("a\n区間:\n").unwrap_err().contains("2 行目"));
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
        let interval = v.iter().filter(|i| i.kind == ItemKind::Interval).count();
        // 表情 13 + 無表情 1、順に 5（向きは合わせて 14）、区間 2
        assert_eq!(pose, 14);
        assert_eq!(seq.len(), 5);
        assert_eq!(seq.iter().sum::<usize>(), 14);
        assert_eq!(interval, 2);
        assert_eq!(v.last().unwrap().name, "力を抜いた無表情");
    }
}
