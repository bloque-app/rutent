//! A small seeded mutation fuzzer for portable binaries (`B-09`, `THR-85`): the same seed gives the same binaries, so a
//! crash found in CI is made again on any machine from its seed. No external fuzzing crate: nothing enters
//! `Cargo.lock` or `licences/RECORD`.

#![allow(dead_code)]

/// SplitMix64: a seeded generator that needs nothing but a `u64`.
pub struct Rng(pub u64);

impl Rng {
    /// The next 64 bits.
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (`n` of at least 1).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Values a parser tends to trip over: the edges of LEB128 and of the integer widths.
const EDGES: [&[u8]; 8] = [
    &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F],
    &[0x80, 0x80, 0x80, 0x80, 0x00],
    &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
    &[0x00],
    &[0x7F],
    &[0xFF, 0xFF, 0xFF, 0x7F],
    &[0x00, 0x00, 0x00, 0x80],
    &[0xFF; 4],
];

/// The byte ranges of a binary's top-level sections, after its eight-byte header, as far as the lengths hold.
pub fn sections(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 8usize.min(bytes.len());
    while at < bytes.len() {
        let start = at;
        at += 1;
        let mut size = 0usize;
        let mut shift = 0;
        loop {
            let Some(&byte) = bytes.get(at) else {
                return found;
            };
            at += 1;
            size |= usize::from(byte & 0x7F).checked_shl(shift).unwrap_or(0);
            shift += 7;
            if byte & 0x80 == 0 || shift > 28 {
                break;
            }
        }
        let end = at.saturating_add(size);
        if end > bytes.len() {
            return found;
        }
        found.push((start, end));
        at = end;
    }
    found
}

/// `seed` mutated by one to six steps, never longer than `cap` bytes.
pub fn mutate(rng: &mut Rng, seed: &[u8], cap: usize) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    for _ in 0..=rng.below(6) {
        if bytes.is_empty() {
            bytes.push(rng.next() as u8);
            continue;
        }
        let len = bytes.len();
        match rng.below(10) {
            0 => {
                let at = rng.below(len);
                bytes[at] ^= 1 << rng.below(8);
            }
            1 => {
                let at = rng.below(len);
                bytes[at] = rng.next() as u8;
            }
            2 => bytes.truncate(rng.below(len)),
            3 => {
                let at = rng.below(len);
                let end = (at + 1 + rng.below(64)).min(len);
                bytes.drain(at..end);
            }
            4 => {
                let at = rng.below(len + 1);
                let extra: Vec<u8> = (0..1 + rng.below(16)).map(|_| rng.next() as u8).collect();
                bytes.splice(at..at, extra);
            }
            5 => {
                let at = rng.below(len);
                let end = (at + 1 + rng.below(256)).min(len);
                let copy = bytes[at..end].to_vec();
                let to = rng.below(len + 1);
                bytes.splice(to..to, copy);
            }
            6 => {
                let edge = EDGES[rng.below(EDGES.len())];
                let at = rng.below(len);
                for (i, byte) in edge.iter().enumerate() {
                    if let Some(slot) = bytes.get_mut(at + i) {
                        *slot = *byte;
                    }
                }
            }
            // Structure-aware: a section's length byte, a section dropped, or two sections swapped.
            7..=9 => {
                let found = sections(&bytes);
                if found.is_empty() {
                    continue;
                }
                let (start, end) = found[rng.below(found.len())];
                match rng.below(3) {
                    0 => bytes[(start + 1).min(len - 1)] = rng.next() as u8,
                    1 => {
                        bytes.drain(start..end);
                    }
                    _ => {
                        let (other, other_end) = found[rng.below(found.len())];
                        if end <= other {
                            let first = bytes[start..end].to_vec();
                            let second = bytes[other..other_end].to_vec();
                            let mut swapped = bytes[..start].to_vec();
                            swapped.extend(&second);
                            swapped.extend(&bytes[end..other]);
                            swapped.extend(&first);
                            swapped.extend(&bytes[other_end..]);
                            bytes = swapped;
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
        bytes.truncate(cap);
    }
    bytes
}
