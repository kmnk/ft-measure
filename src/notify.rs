//! 音と chatbox の知らせ。

use rosc::{encoder, OscMessage, OscPacket, OscType};
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Measure,
    Moving,
    Next,
    Undo,
    Warn,
    IntervalStart,
    IntervalStop,
    Finish,
}

impl Sound {
    pub fn name(self) -> &'static str {
        match self {
            Sound::Measure => crate::i18n::tr("measure", "測定"),
            Sound::Moving => crate::i18n::tr("moving", "動いていた"),
            Sound::Next => crate::i18n::tr("next", "次へ"),
            Sound::Undo => crate::i18n::tr("undo", "取り消し"),
            Sound::Warn => crate::i18n::tr("warning", "警告"),
            Sound::IntervalStart => crate::i18n::tr("recording start", "区間の開始"),
            Sound::IntervalStop => crate::i18n::tr("recording stop", "区間の終わり"),
            Sound::Finish => crate::i18n::tr("finish", "終わり"),
        }
    }

    /// 鳴らす音の列（高さ Hz、長さ ms）。出来事ごとに聞き分けられるようにする。
    pub fn tones(self) -> &'static [(u32, u32)] {
        match self {
            Sound::Measure => &[(880, 120)],
            Sound::Moving => &[(1320, 70), (1320, 70), (1320, 70)],
            Sound::Next => &[(660, 90), (990, 140)],
            Sound::Undo => &[(440, 220)],
            Sound::Warn => &[(220, 450)],
            Sound::IntervalStart => &[(523, 110), (784, 160)],
            Sound::IntervalStop => &[(784, 110), (523, 160)],
            Sound::Finish => &[(523, 120), (659, 120), (784, 240)],
        }
    }
}

#[cfg(windows)]
fn beep(freq: u32, ms: u32) {
    // kernel32 の Beep。呼んだスレッドを止めるので、音専用のスレッドから呼ぶ。
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::Beep(freq, ms);
    }
}

#[cfg(not(windows))]
fn beep(_freq: u32, ms: u32) {
    // Windows 以外（試験の Mac）では鳴らさず、長さだけ待つ。
    thread::sleep(Duration::from_millis(ms as u64));
}

/// 音を鳴らすスレッドに送る口。
pub struct SoundPlayer {
    tx: Sender<Sound>,
    /// 鳴らした出来事を画面に出す（試験用）
    echo: bool,
}

impl SoundPlayer {
    pub fn new(echo: bool) -> Self {
        let (tx, rx) = mpsc::channel::<Sound>();
        thread::spawn(move || {
            for s in rx {
                for (f, ms) in s.tones() {
                    beep(*f, *ms);
                }
            }
        });
        SoundPlayer { tx, echo }
    }

    pub fn play(&self, s: Sound) {
        if self.echo {
            crate::say(&format!("[{}] {}", crate::i18n::tr("sound", "音"), s.name()));
        }
        let _ = self.tx.send(s);
    }
}

/// chatbox に送る文を 144 文字・1 行に収める。
pub fn fit_chatbox(text: &str) -> String {
    let one_line: String = text.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    one_line.chars().take(144).collect()
}

/// chatbox への送り口。前に送ってから `interval` 経つまでは最後の文だけを持っておく。
pub struct Chatbox {
    sock: UdpSocket,
    dest: SocketAddr,
    interval: Duration,
    last_sent: Option<Instant>,
    pending: Option<String>,
}

impl Chatbox {
    pub fn new(dest: SocketAddr, interval: Duration) -> std::io::Result<Self> {
        let sock = UdpSocket::bind("127.0.0.1:0")?;
        Ok(Chatbox { sock, dest, interval, last_sent: None, pending: None })
    }

    pub fn set(&mut self, text: &str, now: Instant) {
        self.pending = Some(fit_chatbox(text));
        self.tick(now);
    }

    pub fn tick(&mut self, now: Instant) {
        if self.pending.is_none() {
            return;
        }
        if let Some(t) = self.last_sent {
            if now.duration_since(t) < self.interval {
                return;
            }
        }
        let text = self.pending.take().unwrap();
        let msg = OscPacket::Message(OscMessage {
            addr: "/chatbox/input".into(),
            args: vec![OscType::String(text), OscType::Bool(true), OscType::Bool(false)],
        });
        if let Ok(bytes) = encoder::encode(&msg) {
            let _ = self.sock.send_to(&bytes, self.dest);
        }
        self.last_sent = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_fit_chatbox_cuts_to_144_one_line() {
        let long = "あ".repeat(200);
        assert_eq!(fit_chatbox(&long).chars().count(), 144);
        assert_eq!(fit_chatbox("a\nb"), "a b");
    }

    #[test]
    fn notify_chatbox_keeps_only_latest_during_interval() {
        let rx = UdpSocket::bind("127.0.0.1:19131").unwrap();
        rx.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        let mut c = Chatbox::new("127.0.0.1:19131".parse().unwrap(), Duration::from_millis(1000)).unwrap();
        let t0 = Instant::now();
        c.set("一", t0);
        c.set("二", t0 + Duration::from_millis(100));
        c.set("三", t0 + Duration::from_millis(200));
        c.tick(t0 + Duration::from_millis(1100));
        let mut got = Vec::new();
        let mut buf = [0u8; 2048];
        while let Ok((n, _)) = rx.recv_from(&mut buf) {
            if let Ok((_, OscPacket::Message(m))) = rosc::decoder::decode_udp(&buf[..n]) {
                assert_eq!(m.addr, "/chatbox/input");
                assert_eq!(m.args[1], OscType::Bool(true));
                assert_eq!(m.args[2], OscType::Bool(false));
                if let OscType::String(s) = &m.args[0] {
                    got.push(s.clone());
                }
            }
        }
        assert_eq!(got, vec!["一".to_string(), "三".to_string()]);
    }
}
