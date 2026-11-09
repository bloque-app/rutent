//! Property tests of the output check's invariants (`DEC-18`, `REQ-MDL-12`): seeded, so a failure names the seed that
//! replays it.

use rutent_contracts::module_interface::PlatformRefusal;
use rutent_contracts::provider_role::ResultBody;
use rutent_prov::module::output::{check, trap_reason};
use serde_json::{Value, json};

const CASES: u64 = 400;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn honest(amount_in: &str, reference: &str) -> Value {
    json!({
        "kind": "withdrawal",
        "transaction": "tx-1",
        "outcome": {
            "status": "succeeded",
            "amount_in": amount_in,
            "amount_out": "1000",
            "fee": { "asset": "COP", "amount": "0" },
            "evidence": [{ "grade": 3, "reference": reference, "by": "acme-pay" }],
        },
    })
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

/// Random text from a small alphabet that mixes digits, a leading zero, signs and letters.
fn text(rng: &mut Rng, longest: u64) -> String {
    const ALPHABET: &[u8] = b"0123456789000123456789-+.eaZ _";
    let length = rng.below(longest + 1);
    (0..length)
        .map(|_| ALPHABET[rng.below(ALPHABET.len() as u64) as usize] as char)
        .collect()
}

fn is_amount(s: &str) -> bool {
    s.len() <= 78
        && !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'))
}

// verifies: REQ-MDL-12
#[test]
fn the_check_passes_an_amount_exactly_when_the_schemas_amount_pattern_and_length_do() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let longest = if seed % 4 == 0 { 100 } else { 6 };
        let amount = text(&mut rng, longest);
        let verdict = check(&bytes(&honest(&amount, "ref-1")), 1 << 20);
        assert_eq!(
            verdict.is_ok(),
            is_amount(&amount),
            "seed {seed}: {amount:?}"
        );
    }
}

// verifies: REQ-MDL-12
#[test]
fn the_check_passes_a_reference_exactly_when_its_length_is_within_the_schemas() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let length = rng.below(600) as usize;
        let reference = "r".repeat(length);
        let verdict = check(&bytes(&honest("1", &reference)), 1 << 20);
        assert_eq!(
            verdict.is_ok(),
            (1..=512).contains(&length),
            "seed {seed}: {length}"
        );
    }
}

// verifies: REQ-MDL-12
#[test]
fn what_the_check_passes_is_the_value_it_judged() {
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let amount = text(&mut rng, 5);
        let raw = bytes(&honest(&amount, "ref-1"));
        let Ok(body) = check(&raw, 1 << 20) else {
            continue;
        };
        let again: Value = serde_json::to_value(&body).unwrap();
        assert_eq!(
            again,
            serde_json::from_slice::<Value>(&raw).unwrap(),
            "seed {seed}"
        );
        assert!(check(&bytes(&again), 1 << 20).is_ok(), "seed {seed}");
    }
}

// verifies: REQ-MDL-12
#[test]
fn the_check_refuses_arbitrary_bytes_without_panicking_and_over_the_limit_as_oversized() {
    let sample = bytes(&honest("1", "ref-1"));
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let mut noise: Vec<u8> = sample.clone();
        for _ in 0..=rng.below(4) {
            let at = rng.below(noise.len() as u64) as usize;
            match rng.below(3) {
                0 => noise[at] = rng.next() as u8,
                1 => {
                    noise.remove(at);
                }
                _ => noise.insert(at, rng.next() as u8),
            }
        }
        let verdict = check(&noise, 1 << 20);
        assert!(
            matches!(verdict, Ok(_) | Err(PlatformRefusal::MalformedOutput)),
            "seed {seed}"
        );
        assert_eq!(
            check(&noise, noise.len() - 1).err(),
            Some(PlatformRefusal::OversizedOutput),
            "seed {seed}"
        );
        let _: Option<ResultBody> = check(&noise, noise.len()).ok();
    }
}

// verifies: REQ-MDL-12, REQ-MDL-9
#[test]
fn a_trap_reason_never_carries_what_the_guest_named() {
    let stopped = trap_reason(&wasmtime::Error::from(wasmtime::Trap::OutOfFuel));
    assert!(stopped.contains("fuel"), "{stopped}");
    for seed in 0..CASES {
        let mut rng = Rng::new(seed);
        let named = format!("guest-{}-{}", rng.next(), text(&mut rng, 8));
        let error = wasmtime::Error::msg(named.clone()).context(format!("in {named}"));
        assert!(!trap_reason(&error).contains("guest-"), "seed {seed}");
    }
}
