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

fn leb(mut n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// The sections of `bytes` after a `header` of that many bytes: id, body, as far as the lengths hold.
fn split(bytes: &[u8], header: usize) -> Option<Vec<(u8, Vec<u8>)>> {
    let mut found = Vec::new();
    let mut at = header;
    if bytes.len() < header {
        return None;
    }
    while at < bytes.len() {
        let id = bytes[at];
        let (start, end) = {
            let mut i = at + 1;
            let mut size = 0usize;
            let mut shift = 0;
            loop {
                let byte = *bytes.get(i)?;
                i += 1;
                size |= usize::from(byte & 0x7F).checked_shl(shift)?;
                shift += 7;
                if byte & 0x80 == 0 {
                    break;
                }
                if shift > 28 {
                    return None;
                }
            }
            (i, i.checked_add(size)?)
        };
        found.push((id, bytes.get(start..end)?.to_vec()));
        at = end;
    }
    Some(found)
}

fn join(header: &[u8], sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = header.to_vec();
    for (id, body) in sections {
        out.push(*id);
        out.extend(leb(body.len()));
        out.extend(body);
    }
    out
}

/// Edits bytes inside `body` (a section's body, or a function's), without breaking the framing around it.
fn edit(rng: &mut Rng, body: &mut Vec<u8>) {
    if body.is_empty() {
        body.push(rng.next() as u8);
        return;
    }
    let len = body.len();
    match rng.below(5) {
        0 => body[rng.below(len)] ^= 1 << rng.below(8),
        1 => body[rng.below(len)] = rng.next() as u8,
        2 => {
            let edge = EDGES[rng.below(EDGES.len())];
            let at = rng.below(len);
            for (i, byte) in edge.iter().enumerate() {
                if let Some(slot) = body.get_mut(at + i) {
                    *slot = *byte;
                }
            }
        }
        3 => {
            let at = rng.below(len);
            body.remove(at);
        }
        _ => {
            let at = rng.below(len + 1);
            body.insert(at, rng.next() as u8);
        }
    }
}

/// Changes one ASCII letter of `body` for another: inside names, which keeps a section well-formed and changes what
/// the compiler is asked to link or export. `false` if the body holds no letter.
fn rename(rng: &mut Rng, body: &mut [u8]) -> bool {
    let letters: Vec<usize> = (0..body.len())
        .filter(|&i| body[i].is_ascii_alphabetic())
        .collect();
    if letters.is_empty() {
        return false;
    }
    body[letters[rng.below(letters.len())]] = b'a' + rng.below(26) as u8;
    true
}

/// Mutates inside the component: one of its nested core modules' sections (types, imports, functions, code, data,
/// and so on), the module and the component re-framed so that the lengths still hold and the compiler reads on into
/// the changed section. `None` if `bytes` has no nested core module.
pub fn mutate_nested(rng: &mut Rng, bytes: &[u8]) -> Option<Vec<u8>> {
    let mut outer = split(bytes, 8)?;
    let modules: Vec<usize> = (0..outer.len()).filter(|&i| outer[i].0 == 1).collect();
    if modules.is_empty() {
        return None;
    }
    if rng.below(4) == 0 {
        let mut body = vec![4, b'f', b'u', b'z', b'z'];
        body.extend((0..rng.below(48)).map(|_| rng.next() as u8));
        let at = rng.below(outer.len() + 1);
        outer.insert(at, (0, body));
        return Some(join(&bytes[..8], &outer));
    }
    // A third of the cases rename something in one of the component's own sections (its types, imports and
    // exports name the worlds the compiler checks a part against).
    if rng.below(3) == 0 {
        let others: Vec<usize> = (0..outer.len()).filter(|&i| outer[i].0 != 1).collect();
        if !others.is_empty() {
            let at = others[rng.below(others.len())];
            if rename(rng, &mut outer[at].1) {
                return Some(join(&bytes[..8], &outer));
            }
        }
    }
    let at = modules[rng.below(modules.len())];
    let module = outer[at].1.clone();
    let mut inner = split(&module, 8)?;
    if inner.is_empty() {
        return None;
    }
    let section = rng.below(inner.len());
    // Edits that keep a module valid come first, so that the case reaches the compiler's own checks: a custom
    // section added or filled with other bytes, data changed in place, a constant in a function body changed, a
    // name's letter swapped. The rest is bytewise, imports, types and code framing included.
    let id = inner[section].0;
    match if rng.below(8) == 0 { 7 } else { rng.below(8) } {
        0 | 1 => {
            let mut body = vec![4, b'f', b'u', b'z', b'z'];
            body.extend((0..rng.below(48)).map(|_| rng.next() as u8));
            let at = rng.below(inner.len() + 1);
            inner.insert(at, (0, body));
        }
        2 | 7 if id == 0 && inner[section].1.len() > 5 => {
            let body = &mut inner[section].1;
            let at = 5 + rng.below(body.len() - 5);
            body[at] = rng.next() as u8;
        }
        3 | 4 | 7 if id == 11 && !inner[section].1.is_empty() => {
            let body = &mut inner[section].1;
            let last = body.len() - 1;
            body[last - rng.below(last.min(8) + 1)] = rng.next() as u8;
        }
        5..=7 if id == 10 => {
            let body = &mut inner[section].1;
            let consts: Vec<usize> = (0..body.len().saturating_sub(1))
                .filter(|&i| body[i] == 0x41 && body[i + 1] < 0x40)
                .collect();
            if consts.is_empty() {
                edit(rng, body);
            } else {
                body[consts[rng.below(consts.len())] + 1] = rng.below(0x40) as u8;
            }
        }
        7 if matches!(id, 2 | 7) && rng.below(2) == 0 => {
            if !rename(rng, &mut inner[section].1) {
                edit(rng, &mut inner[section].1);
            }
        }
        _ if rng.below(3) == 0 => edit(rng, &mut inner[section].1),
        _ => {
            let mut body = vec![4, b'f', b'u', b'z', b'z'];
            body.extend((0..rng.below(48)).map(|_| rng.next() as u8));
            let at = rng.below(inner.len() + 1);
            inner.insert(at, (0, body));
        }
    }
    outer[at].1 = join(&module[..8], &inner);
    Some(join(&bytes[..8], &outer))
}

/// `seed` mutated by one to six steps, never longer than `cap` bytes.
pub fn mutate(rng: &mut Rng, seed: &[u8], cap: usize) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    // Most cases go inside a nested core module first, so that they reach the compiler's checks and not only the
    // parser's first look.
    if rng.below(100) < 95 {
        for _ in 0..=rng.below(2) / 2 {
            if let Some(changed) = mutate_nested(rng, &bytes) {
                bytes = changed;
            }
        }
        if rng.below(100) < 96 {
            bytes.truncate(cap);
            return bytes;
        }
    }
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
