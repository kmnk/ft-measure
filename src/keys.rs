//! キーの読み方。窓（利用者）と台本（試験）を差し替えられる。

use crate::session::Key;
use rosc::{encoder, OscMessage, OscPacket, OscType};
use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
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

/// 窓から届いた入力を読む。窓が閉じられて送り手が無くなったら終わる。
pub struct ChannelKeys(pub mpsc::Receiver<Input>);

impl KeySource for ChannelKeys {
    fn poll(&mut self, timeout: Duration) -> Vec<Input> {
        let mut out = Vec::new();
        match self.0.recv_timeout(timeout) {
            Ok(i) => out.push(i),
            Err(mpsc::RecvTimeoutError::Timeout) => return out,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                out.push(Input::Quit);
                return out;
            }
        }
        while let Ok(i) = self.0.try_recv() {
            out.push(i);
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

    #[test]
    fn keys_channel_returns_all_and_quits_when_closed() {
        let (tx, rx) = mpsc::channel();
        let mut k = ChannelKeys(rx);
        assert!(k.poll(Duration::from_millis(10)).is_empty());
        tx.send(Input::Key(Key::Measure)).unwrap();
        tx.send(Input::Key(Key::Next)).unwrap();
        assert_eq!(k.poll(Duration::from_millis(10)), vec![Input::Key(Key::Measure), Input::Key(Key::Next)]);
        drop(tx);
        assert_eq!(k.poll(Duration::from_millis(10)), vec![Input::Quit]);
    }

    #[test]
    fn keys_script_parse_errors() {
        let t: SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert!(ScriptKeys::parse("measure\nwait x\n", t).is_err());
        assert!(ScriptKeys::parse("jump\n", t).is_err());
        assert!(ScriptKeys::parse("send /a 0.5\nsend /b true\nsend /c 3\n", t).is_ok());
    }
}
