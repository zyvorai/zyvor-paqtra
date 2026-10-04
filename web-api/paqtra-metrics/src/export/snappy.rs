//! Snappy block format (not the framed stream format), as Prometheus remote
//! write requires. A small greedy encoder: a hash table of 4-byte prefixes
//! finds back-references, emitted as copy elements; everything else is
//! literals. The ratio is a bit below the reference encoder but the output
//! is fully compatible.

fn put_uvarint(dst: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        dst.push(v as u8 | 0x80);
        v >>= 7;
    }
    dst.push(v as u8);
}

fn uvarint(src: &[u8]) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for (i, b) in src.iter().enumerate().take(10) {
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

pub fn encode(src: &[u8]) -> Vec<u8> {
    let mut dst = Vec::with_capacity(src.len() / 2 + 16);
    put_uvarint(&mut dst, src.len() as u64);
    if src.len() < 16 {
        emit_literal(&mut dst, src);
        return dst;
    }
    const TABLE_BITS: u32 = 14;
    let mut table = vec![0i32; 1 << TABLE_BITS];
    let hash = |u: u32| (u.wrapping_mul(0x1e35a7bd) >> (32 - TABLE_BITS)) as usize;
    let load = |i: usize| u32::from_le_bytes([src[i], src[i + 1], src[i + 2], src[i + 3]]);
    let (mut lit, mut i) = (0usize, 0usize);
    while i + 4 <= src.len() {
        let h = hash(load(i));
        let cand = table[h] as isize - 1;
        table[h] = (i + 1) as i32;
        if cand < 0 || i - cand as usize > 65535 || load(cand as usize) != load(i) {
            i += 1;
            continue;
        }
        let cand = cand as usize;
        emit_literal(&mut dst, &src[lit..i]);
        let mut n = 4;
        while i + n < src.len() && src[cand + n] == src[i + n] {
            n += 1;
        }
        emit_copy(&mut dst, i - cand, n);
        i += n;
        lit = i;
    }
    emit_literal(&mut dst, &src[lit..]);
    dst
}

fn emit_literal(dst: &mut Vec<u8>, mut lit: &[u8]) {
    while !lit.is_empty() {
        let chunk = &lit[..lit.len().min(65536)];
        let n = chunk.len() - 1;
        if n < 60 {
            dst.push((n as u8) << 2);
        } else if n < 256 {
            dst.extend_from_slice(&[60 << 2, n as u8]);
        } else {
            dst.extend_from_slice(&[61 << 2, n as u8, (n >> 8) as u8]);
        }
        dst.extend_from_slice(chunk);
        lit = &lit[chunk.len()..];
    }
}

/// Copy-2 elements (offset < 65536, length 1..64).
fn emit_copy(dst: &mut Vec<u8>, offset: usize, mut length: usize) {
    while length > 0 {
        let n = length.min(64);
        dst.extend_from_slice(&[((n - 1) as u8) << 2 | 2, offset as u8, (offset >> 8) as u8]);
        length -= n;
    }
}

/// Decodes the block format; used by tests.
pub fn decode(src: &[u8]) -> Result<Vec<u8>, &'static str> {
    const CORRUPT: &str = "snappy: corrupt input";
    let (n, k) = uvarint(src).ok_or(CORRUPT)?;
    if n > 64 << 20 {
        return Err(CORRUPT);
    }
    let mut src = &src[k..];
    let mut dst: Vec<u8> = Vec::with_capacity(n as usize);
    let copy_back = |dst: &mut Vec<u8>, off: usize, l: usize| {
        if off == 0 || off > dst.len() {
            return Err(CORRUPT);
        }
        let start = dst.len() - off;
        for i in 0..l {
            dst.push(dst[start + i]);
        }
        Ok(())
    };
    while let Some(&tag) = src.first() {
        match tag & 3 {
            0 => {
                let mut l = (tag >> 2) as usize;
                src = &src[1..];
                match l {
                    0..=59 => {}
                    60 => {
                        l = *src.first().ok_or(CORRUPT)? as usize;
                        src = &src[1..];
                    }
                    61 => {
                        if src.len() < 2 {
                            return Err(CORRUPT);
                        }
                        l = u16::from_le_bytes([src[0], src[1]]) as usize;
                        src = &src[2..];
                    }
                    _ => return Err(CORRUPT),
                }
                l += 1;
                if src.len() < l {
                    return Err(CORRUPT);
                }
                dst.extend_from_slice(&src[..l]);
                src = &src[l..];
            }
            1 => {
                if src.len() < 2 {
                    return Err(CORRUPT);
                }
                let l = ((tag >> 2) & 7) as usize + 4;
                let off = ((tag >> 5) as usize) << 8 | src[1] as usize;
                src = &src[2..];
                copy_back(&mut dst, off, l)?;
            }
            2 => {
                if src.len() < 3 {
                    return Err(CORRUPT);
                }
                let l = (tag >> 2) as usize + 1;
                let off = u16::from_le_bytes([src[1], src[2]]) as usize;
                src = &src[3..];
                copy_back(&mut dst, off, l)?;
            }
            _ => {
                if src.len() < 5 {
                    return Err(CORRUPT);
                }
                let l = (tag >> 2) as usize + 1;
                let off = u32::from_le_bytes([src[1], src[2], src[3], src[4]]) as usize;
                src = &src[5..];
                copy_back(&mut dst, off, l)?;
            }
        }
    }
    if dst.len() as u64 != n {
        return Err(CORRUPT);
    }
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_compresses() {
        let mut big = Vec::new();
        for i in 0..5000 {
            big.extend_from_slice(
                format!("paqtra_system_cpu{{dimension=\"user\"}} {i}\n").as_bytes(),
            );
        }
        for src in [
            b"".to_vec(),
            b"short".to_vec(),
            vec![7u8; 200_000],
            big.clone(),
        ] {
            let enc = encode(&src);
            assert_eq!(decode(&enc).unwrap(), src);
        }
        assert!(encode(&big).len() < big.len() / 3);
        assert!(decode(&[5, 0xff]).is_err());
    }
}
