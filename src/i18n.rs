//! 表示の言語。画面と chatbox の文だけを切り替え、書き出すファイルは英語にする。

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        match s.trim() {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            _ => None,
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::Ja } else { Lang::En }
}

pub fn set_lang(l: Lang) {
    LANG.store(if l == Lang::Ja { 1 } else { 0 }, Ordering::Relaxed);
}

/// 今の言語の方を返す。
pub fn tr<'a>(en: &'a str, ja: &'a str) -> &'a str {
    match lang() {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

/// 英語と日本語の 2 つを持つ名前。書き出しには `en` を使う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub en: String,
    pub ja: String,
}

impl Text {
    pub fn new(en: &str, ja: &str) -> Text {
        Text { en: en.to_string(), ja: ja.to_string() }
    }

    /// 片方しか無い名前。どちらの言語でも同じ文を出す。
    pub fn same(s: &str) -> Text {
        Text::new(s, s)
    }

    pub fn get(&self, l: Lang) -> &str {
        match l {
            Lang::En => &self.en,
            Lang::Ja => &self.ja,
        }
    }

    pub fn show(&self) -> &str {
        self.get(lang())
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Text {
        Text::same(s)
    }
}
