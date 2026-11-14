//! VRChat の OSC の出力を受け、始めの部分に当たるパラメータの最後の値を表に持つ。

use rosc::{OscPacket, OscType};
use std::collections::HashMap;
use std::io;
use std::net::UdpSocket;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

/// キーの名前（始めの部分を除いたもの）→ 最後に届いた値。
pub type Table = Arc<Mutex<HashMap<String, f32>>>;

#[derive(Debug, Clone, PartialEq)]
pub enum OscEvent {
    AvatarChanged(String),
}

pub const PARAM_ROOT: &str = "/avatar/parameters/";

/// 受け口を作る。使われていれば `AddrInUse` の誤りになる。
pub fn bind(host: &str, port: u16) -> io::Result<UdpSocket> {
    UdpSocket::bind((host, port))
}

fn to_f32(v: &OscType) -> Option<f32> {
    match v {
        OscType::Float(f) => Some(*f),
        OscType::Double(d) => Some(*d as f32),
        OscType::Int(i) => Some(*i as f32),
        OscType::Long(l) => Some(*l as f32),
        OscType::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// 1 つの packet を表に反映する。
pub fn apply_packet(packet: &OscPacket, prefix: &str, table: &Table, events: &Sender<OscEvent>) {
    match packet {
        OscPacket::Bundle(b) => {
            for p in &b.content {
                apply_packet(p, prefix, table, events);
            }
        }
        OscPacket::Message(m) => {
            if m.addr == "/avatar/change" {
                table.lock().unwrap().clear();
                let id = match m.args.first() {
                    Some(OscType::String(s)) => s.clone(),
                    _ => String::new(),
                };
                let _ = events.send(OscEvent::AvatarChanged(id));
                return;
            }
            let Some(rest) = m.addr.strip_prefix(PARAM_ROOT) else { return };
            let Some(key) = rest.strip_prefix(prefix) else { return };
            if key.is_empty() {
                return;
            }
            if let Some(v) = m.args.first().and_then(to_f32) {
                table.lock().unwrap().insert(key.to_string(), v);
            }
        }
    }
}

/// 受けるスレッドを始める。
pub fn spawn(socket: UdpSocket, prefix: String, table: Table, events: Sender<OscEvent>) {
    thread::spawn(move || {
        let mut buf = [0u8; 65536];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((n, _)) => {
                    if let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..n]) {
                        apply_packet(&packet, &prefix, &table, &events);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(_) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{encoder, OscBundle, OscMessage, OscTime};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn msg(addr: &str, arg: OscType) -> OscPacket {
        OscPacket::Message(OscMessage { addr: addr.into(), args: vec![arg] })
    }

    fn wait_for(table: &Table, f: impl Fn(&HashMap<String, f32>) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if f(&table.lock().unwrap()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn osc_receives_prefixed_values_only() {
        let sock = bind("127.0.0.1", 19121).unwrap();
        let table: Table = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel();
        spawn(sock, "FT/In/".into(), table.clone(), tx);
        let out = UdpSocket::bind("127.0.0.1:0").unwrap();
        let send = |p: &OscPacket| {
            out.send_to(&encoder::encode(p).unwrap(), "127.0.0.1:19121").unwrap();
        };
        send(&msg("/avatar/parameters/FT/In/JawOpen", OscType::Float(0.5)));
        send(&OscPacket::Bundle(OscBundle {
            timetag: OscTime { seconds: 0, fractional: 1 },
            content: vec![
                msg("/avatar/parameters/FT/In/A", OscType::Bool(true)),
                msg("/avatar/parameters/FT/In/B", OscType::Int(3)),
            ],
        }));
        send(&msg("/avatar/parameters/GestureLeft", OscType::Int(2)));
        assert!(wait_for(&table, |t| t.len() == 3));
        {
            let t = table.lock().unwrap();
            assert_eq!(t.get("JawOpen"), Some(&0.5));
            assert_eq!(t.get("A"), Some(&1.0));
            assert_eq!(t.get("B"), Some(&3.0));
            assert!(!t.keys().any(|k| k.contains("Gesture")));
        }
        send(&msg("/avatar/change", OscType::String("avtr_x".into())));
        assert!(wait_for(&table, |t| t.is_empty()));
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), OscEvent::AvatarChanged("avtr_x".into()));
    }

    #[test]
    fn osc_bind_reports_port_in_use() {
        let _a = bind("127.0.0.1", 19122).unwrap();
        let e = bind("127.0.0.1", 19122).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AddrInUse);
    }
}
