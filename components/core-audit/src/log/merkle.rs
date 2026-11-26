//! The log's Merkle tree, as RFC 9162 §2.1 defines it: a leaf's hash is SHA-256 of `0x00` followed by the leaf's
//! bytes, an inner node's is SHA-256 of `0x01` followed by its two children, and a tree of `n` leaves splits at the
//! largest power of two below `n`. The leaves are the entries' RFC 8785 canonical JSON, the bytes a receipt's digest
//! is taken over, so a checker holding the history recomputes every root (ADR-0005, `DEC-12`).

use sha2::{Digest as _, Sha256};

/// A SHA-256 hash.
pub type Hash = [u8; 32];

/// SHA-256 of `bytes`.
pub fn sha256(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

/// The hash of a leaf: SHA-256 of `0x00` followed by its bytes (RFC 9162 §2.1.1).
pub fn leaf_hash(leaf: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update([0u8]);
    hasher.update(leaf);
    hasher.finalize().into()
}

/// The hash of an inner node: SHA-256 of `0x01` followed by its two children.
fn node_hash(left: &Hash, right: &Hash) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update([1u8]);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// The largest power of two below `n`, for `n` above 1.
fn split(n: usize) -> usize {
    let mut k = 1;
    while k << 1 < n {
        k <<= 1;
    }
    k
}

/// The root of the tree over `leaves`, each given by its leaf hash: the hash of no bytes for no leaves.
pub fn root(leaves: &[Hash]) -> Hash {
    match leaves.len() {
        0 => sha256(&[]),
        1 => leaves[0],
        n => {
            let k = split(n);
            node_hash(&root(&leaves[..k]), &root(&leaves[k..]))
        }
    }
}

/// The inclusion proof of leaf `index` in the tree over `leaves` (RFC 9162 §2.1.3.1), innermost sibling first, or
/// `None` where the tree has no such leaf.
pub fn inclusion_proof(index: usize, leaves: &[Hash]) -> Option<Vec<Hash>> {
    if index >= leaves.len() {
        return None;
    }
    let mut proof = Vec::new();
    path(index, leaves, &mut proof);
    Some(proof)
}

fn path(index: usize, leaves: &[Hash], proof: &mut Vec<Hash>) {
    let n = leaves.len();
    if n <= 1 {
        return;
    }
    let k = split(n);
    if index < k {
        path(index, &leaves[..k], proof);
        proof.push(root(&leaves[k..]));
    } else {
        path(index - k, &leaves[k..], proof);
        proof.push(root(&leaves[..k]));
    }
}

/// Whether `proof` shows the leaf hashed `leaf` at `index` in a tree of `size` leaves whose root is `root`
/// (RFC 9162 §2.1.3.2).
pub fn verify_inclusion(
    leaf: &Hash,
    index: usize,
    size: usize,
    proof: &[Hash],
    root: &Hash,
) -> bool {
    if index >= size {
        return false;
    }
    let (mut fn_, mut sn) = (index, size - 1);
    let mut r = *leaf;
    for p in proof {
        if sn == 0 {
            return false;
        }
        if fn_ & 1 == 1 || fn_ == sn {
            r = node_hash(p, &r);
            if fn_ & 1 == 0 {
                while fn_ & 1 == 0 && fn_ != 0 {
                    fn_ >>= 1;
                    sn >>= 1;
                }
            }
        } else {
            r = node_hash(&r, p);
        }
        fn_ >>= 1;
        sn >>= 1;
    }
    sn == 0 && r == *root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves(n: usize) -> Vec<Hash> {
        (0..n).map(|i| leaf_hash(&[i as u8, 7])).collect()
    }

    /// The root of the RFC 9162 tree is the textbook one: two leaves join under `0x01`, three split as two and one.
    // verifies: DEC-12
    #[test]
    fn the_root_is_rfc_9162s_tree_hash() {
        let l = leaves(3);
        assert_eq!(root(&[]), sha256(&[]));
        assert_eq!(root(&l[..1]), l[0]);
        assert_eq!(root(&l[..2]), node_hash(&l[0], &l[1]));
        assert_eq!(root(&l), node_hash(&node_hash(&l[0], &l[1]), &l[2]));
        assert_ne!(leaf_hash(b"x"), sha256(b"x"));
    }

    /// Every leaf of every tree up to 33 leaves has a proof that verifies, and no proof verifies a wrong leaf, a
    /// wrong place or a wrong root.
    // verifies: DEC-12
    #[test]
    fn every_leaf_has_an_inclusion_proof_and_no_other_leaf_passes_it() {
        for n in 1..=33 {
            let l = leaves(n);
            let r = root(&l);
            for i in 0..n {
                let proof = inclusion_proof(i, &l).unwrap();
                assert!(verify_inclusion(&l[i], i, n, &proof, &r), "{i} of {n}");
                assert!(!verify_inclusion(&leaf_hash(b"other"), i, n, &proof, &r));
                if n > 1 {
                    assert!(!verify_inclusion(&l[i], (i + 1) % n, n, &proof, &r));
                }
                assert!(!verify_inclusion(&l[i], i, n, &proof, &sha256(b"root")));
            }
            assert_eq!(inclusion_proof(n, &l), None);
        }
    }
}
