//! The address check of the binding's egress layer (ADR-0004): whether a connected address is within a
//! destination's declared addresses. A port of the module-boundary suite's reference double, whose copy is
//! private, against the contract's `SPECIAL_PURPOSE` ranges.

use std::net::IpAddr;

use rutent_contracts::module_interface::{Addresses, SPECIAL_PURPOSE};

/// Whether `address` is within `addresses`: for `public`, globally routable, in no special-purpose range.
pub fn allows(addresses: &Addresses, address: IpAddr) -> bool {
    match addresses {
        Addresses::Public(_) => !SPECIAL_PURPOSE.iter().any(|(range, len)| {
            range
                .parse::<IpAddr>()
                .is_ok_and(|range| within(address, range, *len))
        }),
        Addresses::Blocks(blocks) => blocks.iter().any(|block| {
            let (ip, len) = block.parts();
            within(address, ip, len)
        }),
    }
}

/// Whether the address `ip` lies within the block `outer/outer_len`.
fn within(ip: IpAddr, outer: IpAddr, outer_len: u8) -> bool {
    let bits = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)) << 96, true),
        IpAddr::V6(v6) => (u128::from(v6), false),
    };
    let ((a, a4), (b, b4)) = (bits(ip), bits(outer));
    if a4 != b4 {
        return false;
    }
    let mask = if outer_len == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(outer_len))
    };
    a & mask == b & mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn public() -> Addresses {
        serde_json::from_value(serde_json::json!("public")).expect("public")
    }

    // verifies: REQ-MDL-7, ADR-0004
    #[test]
    fn public_takes_a_global_address_and_refuses_private_loopback_unique_local_and_mapped_ones() {
        let p = public();
        assert!(allows(&p, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 10))));
        assert!(!allows(&p, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(!allows(&p, IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!allows(
            &p,
            IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 5))
        ));
        assert!(!allows(
            &p,
            IpAddr::V6(Ipv4Addr::new(10, 0, 0, 5).to_ipv6_mapped())
        ));
    }

    // verifies: REQ-MDL-7, ADR-0004
    #[test]
    fn a_pinned_block_takes_only_its_own_addresses() {
        let pinned: Addresses =
            serde_json::from_value(serde_json::json!(["185.12.64.13/32"])).expect("blocks");
        assert!(allows(&pinned, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 13))));
        assert!(!allows(&pinned, IpAddr::V4(Ipv4Addr::new(185, 12, 64, 19))));
    }
}
