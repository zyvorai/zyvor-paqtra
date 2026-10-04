//! Tier-0 chunk: delta-of-delta timestamps, XOR-encoded floats and one
//! anomaly bit per sample.

/// Samples per chunk. At 1s resolution one chunk covers four minutes.
pub const CHUNK_CAP: usize = 240;

#[derive(Default, Clone)]
struct BitWriter {
    b: Vec<u8>,
    free: u8,
}

impl BitWriter {
    fn write_bit(&mut self, on: bool) {
        if self.free == 0 {
            self.b.push(0);
            self.free = 8;
        }
        if on {
            let last = self.b.len() - 1;
            self.b[last] |= 1 << (self.free - 1);
        }
        self.free -= 1;
    }

    fn write_bits(&mut self, v: u64, mut n: u32) {
        while n > 0 {
            if self.free == 0 {
                self.b.push(0);
                self.free = 8;
            }
            let take = n.min(self.free as u32);
            let shift = n - take;
            let mask = if take == 64 {
                u64::MAX
            } else {
                (1u64 << take) - 1
            };
            let part = ((v >> shift) & mask) as u8;
            let last = self.b.len() - 1;
            self.b[last] |= part << (self.free as u32 - take);
            self.free -= take as u8;
            n -= take;
        }
    }
}

struct BitReader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl BitReader<'_> {
    fn read_bit(&mut self) -> bool {
        if self.pos >= self.b.len() * 8 {
            return false;
        }
        let v = self.b[self.pos / 8] & (1 << (7 - (self.pos % 8))) != 0;
        self.pos += 1;
        v
    }

    fn read_bits(&mut self, n: u32) -> u64 {
        let mut v = 0u64;
        for _ in 0..n {
            v <<= 1;
            if self.read_bit() {
                v |= 1;
            }
        }
        v
    }
}

#[derive(Clone)]
pub struct Chunk {
    pub first: i64,
    pub last: i64,
    pub n: usize,
    w: BitWriter,
    prev_delta: i64,
    prev_v: u64,
    leading: u8,
    trailing: u8,
    anom: [u64; CHUNK_CAP.div_ceil(64)],
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            first: 0,
            last: 0,
            n: 0,
            w: BitWriter::default(),
            prev_delta: 0,
            prev_v: 0,
            leading: 0xff,
            trailing: 0,
            anom: [0; CHUNK_CAP.div_ceil(64)],
        }
    }
}

impl Chunk {
    pub fn full(&self) -> bool {
        self.n >= CHUNK_CAP
    }

    pub fn append(&mut self, t: i64, v: f64, anomalous: bool) {
        let vb = v.to_bits();
        match self.n {
            0 => {
                self.first = t;
                self.w.write_bits(t as u64, 64);
                self.w.write_bits(vb, 64);
                self.leading = 0xff;
            }
            1 => {
                let delta = t - self.last;
                self.w.write_bits(delta as u64, 32);
                self.prev_delta = delta;
                self.write_value(vb);
            }
            _ => {
                let delta = t - self.last;
                let dod = delta - self.prev_delta;
                if dod == 0 {
                    self.w.write_bit(false);
                } else if (-63..=64).contains(&dod) {
                    self.w.write_bits(0b10, 2);
                    self.w.write_bits(dod as u64 & 0x7f, 7);
                } else if (-255..=256).contains(&dod) {
                    self.w.write_bits(0b110, 3);
                    self.w.write_bits(dod as u64 & 0x1ff, 9);
                } else if (-2047..=2048).contains(&dod) {
                    self.w.write_bits(0b1110, 4);
                    self.w.write_bits(dod as u64 & 0xfff, 12);
                } else {
                    self.w.write_bits(0b1111, 4);
                    self.w.write_bits(dod as u64, 64);
                }
                self.prev_delta = delta;
                self.write_value(vb);
            }
        }
        if anomalous {
            self.anom[self.n / 64] |= 1 << (self.n % 64);
        }
        self.prev_v = vb;
        self.last = t;
        self.n += 1;
    }

    fn write_value(&mut self, vb: u64) {
        let x = vb ^ self.prev_v;
        if x == 0 {
            self.w.write_bit(false);
            return;
        }
        self.w.write_bit(true);
        let lead = (x.leading_zeros() as u8).min(31);
        let trail = x.trailing_zeros() as u8;
        if self.leading != 0xff && lead >= self.leading && trail >= self.trailing {
            self.w.write_bit(false);
            let sig = 64 - self.leading as u32 - self.trailing as u32;
            self.w.write_bits(x >> self.trailing, sig);
            return;
        }
        self.leading = lead;
        self.trailing = trail;
        self.w.write_bit(true);
        self.w.write_bits(lead as u64, 5);
        let sig = 64 - lead as u32 - trail as u32;
        self.w.write_bits((sig & 63) as u64, 6);
        self.w.write_bits(x >> trail, sig);
    }

    /// Decodes the chunk in order; `f` returning false stops iteration.
    pub fn for_each(&self, mut f: impl FnMut(i64, f64, bool) -> bool) {
        let mut r = BitReader {
            b: &self.w.b,
            pos: 0,
        };
        let (mut t, mut delta, mut vb) = (0i64, 0i64, 0u64);
        let (mut leading, mut trailing) = (0u8, 0u8);
        for i in 0..self.n {
            match i {
                0 => {
                    t = r.read_bits(64) as i64;
                    vb = r.read_bits(64);
                }
                1 => {
                    delta = r.read_bits(32) as u32 as i32 as i64;
                    t += delta;
                    (vb, leading, trailing) = read_value(&mut r, vb, leading, trailing);
                }
                _ => {
                    let dod = if !r.read_bit() {
                        0
                    } else if !r.read_bit() {
                        sign_extend(r.read_bits(7), 7)
                    } else if !r.read_bit() {
                        sign_extend(r.read_bits(9), 9)
                    } else if !r.read_bit() {
                        sign_extend(r.read_bits(12), 12)
                    } else {
                        r.read_bits(64) as i64
                    };
                    delta += dod;
                    t += delta;
                    (vb, leading, trailing) = read_value(&mut r, vb, leading, trailing);
                }
            }
            let anom = self.anom[i / 64] & (1 << (i % 64)) != 0;
            if !f(t, f64::from_bits(vb), anom) {
                return;
            }
        }
    }

    pub fn size_bytes(&self) -> usize {
        self.w.b.capacity() + 96
    }
}

fn read_value(r: &mut BitReader, prev: u64, mut leading: u8, mut trailing: u8) -> (u64, u8, u8) {
    if !r.read_bit() {
        return (prev, leading, trailing);
    }
    if r.read_bit() {
        let l = r.read_bits(5) as u8;
        let mut sig = r.read_bits(6) as u32;
        if sig == 0 {
            sig = 64;
        }
        leading = l;
        trailing = (64 - l as u32 - sig) as u8;
    }
    let sig = 64 - leading as u32 - trailing as u32;
    let x = r.read_bits(sig);
    (prev ^ (x << trailing), leading, trailing)
}

/// Interprets the low `n` bits of `v` as a value in [-(2^(n-1)-1), 2^(n-1)].
fn sign_extend(v: u64, n: u32) -> i64 {
    let half = 1u64 << (n - 1);
    if v > half {
        v as i64 - (1i64 << n)
    } else {
        v as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(points: &[(i64, f64, bool)]) {
        let mut c = Chunk::default();
        for &(t, v, a) in points {
            c.append(t, v, a);
        }
        let mut got = Vec::new();
        c.for_each(|t, v, a| {
            got.push((t, v, a));
            true
        });
        assert_eq!(got.len(), points.len());
        for (g, w) in got.iter().zip(points) {
            assert_eq!(g.0, w.0);
            assert!(
                g.1 == w.1 || (g.1.is_nan() && w.1.is_nan()),
                "{g:?} != {w:?}"
            );
            assert_eq!(g.2, w.2);
        }
    }

    #[test]
    fn regular_seconds_and_varied_values() {
        let pts: Vec<_> = (0..CHUNK_CAP as i64)
            .map(|i| {
                (
                    1_700_000_000 + i,
                    (i as f64 * 0.37).sin() * 1e3,
                    i % 17 == 0,
                )
            })
            .collect();
        roundtrip(&pts);
    }

    #[test]
    fn gaps_negative_values_and_constants() {
        let mut pts = vec![(100, 5.0, false), (101, 5.0, false), (103, -2.5, true)];
        pts.push((200, f64::MAX, false));
        pts.push((5000, 0.0, false));
        pts.push((5001, 1e-300, false));
        pts.push((5065, -0.0, false));
        pts.push((5321, f64::NAN, false));
        pts.push((7369, 42.0, true));
        roundtrip(&pts);
    }

    #[test]
    fn constant_series_compresses() {
        let mut c = Chunk::default();
        for i in 0..CHUNK_CAP as i64 {
            c.append(1000 + i, 7.0, false);
        }
        // 21-byte header, then two bits per sample.
        assert!(c.w.b.len() <= 82, "{} bytes", c.w.b.len());
    }
}
