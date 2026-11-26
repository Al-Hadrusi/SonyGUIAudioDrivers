//! Randomised robustness tests: whatever bytes a (malicious or broken)
//! headset sends, decoding must never panic or overflow, and the session
//! must keep working.

use headphones_control::protocol::*;
use headphones_control::transport::Transport;
use std::io;
use std::time::Duration;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn byte(&mut self) -> u8 {
        (self.next() & 0xff) as u8
    }
}

fn dialects() -> Vec<Dialect> {
    vec![
        Dialect::V1,
        Dialect::v2(AmbientType::Basic),
        Dialect::v2(AmbientType::WithWind),
        Dialect::v2(AmbientType::NoiseAdaptive),
    ]
}

#[test]
fn fuzz_parsers() {
    let mut r = Rng(0x9E3779B97F4A7C15);
    let ds = dialects();
    for _ in 0..300_000 {
        let len = (r.next() % 24) as usize;
        let mut p: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        // bias the opcode/sub towards interesting values
        if len > 1 && r.next().is_multiple_of(2) {
            p[1] = [
                0x01, 0x02, 0x03, 0x04, 0x15, 0x17, 0x19, 0x20, 0x30, 0xD1, 0xD2, 0xD4,
            ][(r.next() % 12) as usize];
        }
        for d in &ds {
            let _ = Notification::parse(&p, *d);
            let _ = Notification::parse_t2(&p, *d);
            let _ = Request::decode(&p, *d);
            let _ = Request::decode_t2(&p, *d);
            let _ = Notification::ambient_type_of(&p);
            if let Ok(n) = Notification::parse(&p, *d) {
                let _ = n.payload(*d);
            }
        }
    }
    let mut dec = FrameDecoder::new();
    for _ in 0..50_000 {
        let len = (r.next() % 64) as usize;
        let chunk: Vec<u8> = (0..len)
            .map(|_| {
                if r.next().is_multiple_of(6) {
                    [0x3C, 0x3D, 0x3E][(r.next() % 3) as usize]
                } else {
                    r.byte()
                }
            })
            .collect();
        let _ = dec.push(&chunk);
    }
}

struct Evil {
    r: Rng,
    writes: usize,
}
impl Transport for Evil {
    fn write_all(&mut self, b: &[u8]) -> io::Result<()> {
        self.writes += b.len();
        Ok(())
    }
    fn read(&mut self, buf: &mut [u8], _t: Duration) -> io::Result<usize> {
        // Emit valid frames with random payloads, plus junk.
        let mut out = Vec::new();
        while out.len() < buf.len() - 100 {
            let ty = [0x01u8, 0x0C, 0x0E, 0x55][(self.r.next() % 4) as usize];
            let len = (self.r.next() % 16) as usize;
            let mut p: Vec<u8> = (0..len).map(|_| self.r.byte()).collect();
            if len > 0 && self.r.next().is_multiple_of(2) {
                p[0] = [
                    0x01, 0x03, 0x11, 0x13, 0x19, 0x1B, 0x22, 0x23, 0x57, 0x59, 0x5B, 0x5D, 0x67,
                    0x69, 0xD1, 0xD7, 0xD9, 0xE5, 0xE9, 0xF9,
                ][(self.r.next() % 20) as usize];
            }
            out.extend(Frame::new(DataType::from_code(ty), self.r.byte(), p).encode());
        }
        let n = out.len().min(buf.len());
        buf[..n].copy_from_slice(&out[..n]);
        Ok(n)
    }
}

#[test]
fn fuzz_session() {
    for seed in 1..10u64 {
        let mut s = Session::new(Box::new(Evil {
            r: Rng(seed * 7919),
            writes: 0,
        }));
        for _ in 0..500 {
            let _ = s.poll(Duration::from_millis(0));
            let _ = s.send(&Request::GetBattery(BatteryKind::Single));
        }
    }
}
