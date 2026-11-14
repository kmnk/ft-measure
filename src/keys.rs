//! キーの読み方。端末（利用者）と台本（試験）を差し替えられる。

use crate::session::Key;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rosc::{encoder, OscMessage, OscPacket, OscType};
use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Key(Key),
    Quit,
}

pub trait KeySource {
    /// 最長 `timeout` 待って、届いた入力を返す。
    fn poll(&mut self, timeout: Duration) -> Vec<Input>;
}

/// 端末のキーの出来事を入力にする。押した出来事だけを数える。
/// 半角と全角（U+3000）のスペースを測定とみなす（日本語の IME がオンのときのため）。
pub fn map_key(ev: &KeyEvent) -> Option<Input> {
    if ev.kind != KeyEventKind::Press {
        return None;
    }
    if ev.modifiers.contains(KeyModifiers::CONTROL) && ev.code == KeyCode::Char('c') {
        return Some(Input::Quit);
    }
    match ev.code {
        KeyCode::Char(' ') | KeyCode::Char('\u{3000}') => Some(Input::Key(Key::Measure)),
        KeyCode::Enter => Some(Input::Key(Key::Next)),
        KeyCode::Backspace => Some(Input::Key(Key::Undo)),
        _ => None,
    }
}

pub struct TerminalKeys;

impl TerminalKeys {
    pub fn new() -> std::io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(TerminalKeys)
    }
}

impl Drop for TerminalKeys {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

impl KeySource for TerminalKeys {
    fn poll(&mut self, timeout: Duration) -> Vec<Input> {
        let mut out = Vec::new();
        let mut wait = timeout;
        while let Ok(true) = event::poll(wait) {
            if let Ok(Event::Key(k)) = event::read() {
                if let Some(i) = map_key(&k) {
                    out.push(i);
                }
            }
            wait = Duration::ZERO;
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    Key(Key),
    Wait(u64),
    Send(String, OscType),
    Quit,
}

fn parse_value(s: &str) -> Option<OscType> {
    match s {
        "true" => Some(OscType::Bool(true)),
        "false" => Some(OscType::Bool(false)),
        _ if s.contains('.') => s.parse::<f32>().ok().map(OscType::Float),
        _ => s.parse::<i32>().ok().map(OscType::Int),
    }
}

/// 試験用の台本。1 行に 1 つ、measure・next・undo・quit・wait <ms>・send <アドレス> <値>。
pub struct ScriptKeys {
    cmds: VecDeque<Cmd>,
    resume_at: Instant,
    sock: UdpSocket,
    target: SocketAddr,
}

impl ScriptKeys {
    pub fn parse(text: &str, target: SocketAddr) -> Result<Self, String> {
        let mut cmds = VecDeque::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            let c = match parts.as_slice() {
                ["measure"] => Cmd::Key(Key::Measure),
                ["next"] => Cmd::Key(Key::Next),
                ["undo"] => Cmd::Key(Key::Undo),
                ["quit"] => Cmd::Quit,
                ["wait", ms] => Cmd::Wait(ms.parse().map_err(|_| format!("台本の {} 行目: wait の数", i + 1))?),
                ["send", addr, v] => Cmd::Send(
                    addr.to_string(),
                    parse_value(v).ok_or_else(|| format!("台本の {} 行目: send の値", i + 1))?,
                ),
                _ => return Err(format!("台本の {} 行目を読めません: {}", i + 1, line)),
            };
            cmds.push_back(c);
        }
        let sock = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        Ok(ScriptKeys { cmds, resume_at: Instant::now(), sock, target })
    }
}

impl KeySource for ScriptKeys {
    fn poll(&mut self, timeout: Duration) -> Vec<Input> {
        let now = Instant::now();
        if now < self.resume_at {
            std::thread::sleep(timeout.min(self.resume_at - now));
            return vec![];
        }
        let mut out = Vec::new();
        while let Some(c) = self.cmds.pop_front() {
            match c {
                Cmd::Key(k) => {
                    out.push(Input::Key(k));
                    // 1 回の poll で 1 つのキーだけ返し、間に拾いの時刻を挟めるようにする
                    return out;
                }
                Cmd::Wait(ms) => {
                    self.resume_at = Instant::now() + Duration::from_millis(ms);
                    return out;
                }
                Cmd::Send(addr, v) => {
                    let p = OscPacket::Message(OscMessage { addr, args: vec![v] });
                    if let Ok(b) = encoder::encode(&p) {
                        let _ = self.sock.send_to(&b, self.target);
                    }
                }
                Cmd::Quit => {
                    out.push(Input::Quit);
                    return out;
                }
            }
        }
        // 台本が尽きたら終わる
        out.push(Input::Quit);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn keys_map_space_fullwidth_space_enter_backspace() {
        assert_eq!(map_key(&press(KeyCode::Char(' '))), Some(Input::Key(Key::Measure)));
        assert_eq!(map_key(&press(KeyCode::Char('\u{3000}'))), Some(Input::Key(Key::Measure)));
        assert_eq!(map_key(&press(KeyCode::Enter)), Some(Input::Key(Key::Next)));
        assert_eq!(map_key(&press(KeyCode::Backspace)), Some(Input::Key(Key::Undo)));
        assert_eq!(map_key(&press(KeyCode::Char('a'))), None);
        let mut release = press(KeyCode::Char(' '));
        release.kind = KeyEventKind::Release;
        assert_eq!(map_key(&release), None);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(map_key(&ctrl_c), Some(Input::Quit));
    }

    #[test]
    fn keys_script_parse_errors() {
        let t: SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert!(ScriptKeys::parse("measure\nwait x\n", t).is_err());
        assert!(ScriptKeys::parse("jump\n", t).is_err());
        assert!(ScriptKeys::parse("send /a 0.5\nsend /b true\nsend /c 3\n", t).is_ok());
    }
}
