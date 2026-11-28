//! Core's side of the reader link's TLS (`T-020-60`, `DEC-28`): the operator's CA as the one trust anchor, held to be a
//! CA, and Core's own client certificate and key, on `rustls` with its `ring` provider (no `aws-lc-rs`, no built-in root
//! store). The host's certificate is held to that CA and to the subject name the listing names, never to a pinned leaf.

use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};

/// Why Core's side of the reader link wasn't built from what the deployment handed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsRefusal {
    /// The operator's CA isn't one PEM certificate.
    CaUnreadable,
    /// The certificate handed as the operator's CA is no CA: it has no basic constraints naming it one, as a leaf's
    /// certificate has none, so a leaf put in the CA's place fails closed (#343's review, L3; the CA basic-constraints
    /// check).
    NotACa,
    /// Core's certificate chain isn't PEM, or holds no certificate.
    ChainUnreadable,
    /// Core's key isn't PEM.
    KeyUnreadable,
    /// `rustls` refused the anchor, the chain or the key.
    Refused,
}

/// Core's TLS for the reader link: the client configuration, and the SHA-256 digest of the operator's CA it anchors
/// on, which each listing's `ca` must name.
#[derive(Clone)]
pub struct ReaderTls {
    pub(super) client: Arc<ClientConfig>,
    ca_sha256: String,
}

impl core::fmt::Debug for ReaderTls {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReaderTls")
            .field("ca_sha256", &self.ca_sha256)
            .finish_non_exhaustive()
    }
}

impl ReaderTls {
    /// Core's TLS from what the deployment hands it, as PEM: `ca`, the operator's CA certificate, the one anchor, which
    /// must be a CA by its basic constraints; `chain`, Core's client certificate the CA issued, then any intermediates;
    /// and `key`, its private key.
    ///
    /// # Errors
    ///
    /// Each as [`TlsRefusal`] names it.
    pub fn new(ca: &[u8], chain: &[u8], key: &[u8]) -> Result<Self, TlsRefusal> {
        let mut anchors = CertificateDer::pem_slice_iter(ca);
        let anchor = anchors
            .next()
            .and_then(Result::ok)
            .ok_or(TlsRefusal::CaUnreadable)?;
        if anchors.next().is_some() {
            return Err(TlsRefusal::CaUnreadable);
        }
        if !is_ca(anchor.as_ref()) {
            return Err(TlsRefusal::NotACa);
        }
        let ca_sha256 = rutent_prov::module::wire::sha256(anchor.as_ref());
        let mut roots = RootCertStore::empty();
        roots.add(anchor).map_err(|_| TlsRefusal::Refused)?;
        let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(chain)
            .collect::<Result<_, _>>()
            .map_err(|_| TlsRefusal::ChainUnreadable)?;
        if chain.is_empty() {
            return Err(TlsRefusal::ChainUnreadable);
        }
        let key = PrivateKeyDer::from_pem_slice(key).map_err(|_| TlsRefusal::KeyUnreadable)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut client = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|_| TlsRefusal::Refused)?
            .with_root_certificates(roots)
            .with_client_auth_cert(chain, key)
            .map_err(|_| TlsRefusal::Refused)?;
        client.resumption = rustls::client::Resumption::disabled();
        client.enable_early_data = false;
        Ok(ReaderTls {
            client: Arc::new(client),
            ca_sha256,
        })
    }

    /// The SHA-256 digest of the operator's CA certificate (DER), in lower-case hexadecimal: what a listing's `ca`
    /// must name for Core to ask its reader at all.
    pub fn ca_sha256(&self) -> &str {
        &self.ca_sha256
    }
}

/// Whether the certificate `der` names itself a CA: its basic constraints extension (`2.5.29.19`) is present with
/// `cA` true (RFC 5280 §4.2.1.9). A certificate without the extension, or with `cA` false or absent, is a leaf's.
pub fn is_ca(der: &[u8]) -> bool {
    basic_constraints_ca(der).unwrap_or(false)
}

/// One DER element: its tag, and its contents.
fn element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count]
            .iter()
            .fold(0usize, |n, &b| (n << 8) | usize::from(b));
        (length, &rest[count..])
    };
    if rest.len() < length {
        return None;
    }
    Some((tag, &rest[..length], &rest[length..]))
}

/// The basic constraints' `cA`, where the certificate states the extension: `Some(false)` where it doesn't, and
/// `None` where the certificate doesn't read as DER.
fn basic_constraints_ca(der: &[u8]) -> Option<bool> {
    const SEQUENCE: u8 = 0x30;
    const EXTENSIONS: u8 = 0xa3;
    const OID: u8 = 0x06;
    const BOOLEAN: u8 = 0x01;
    const OCTETS: u8 = 0x04;
    const BASIC_CONSTRAINTS: [u8; 3] = [0x55, 0x1d, 0x13];
    let (tag, certificate, _) = element(der)?;
    if tag != SEQUENCE {
        return None;
    }
    let (tag, mut tbs, _) = element(certificate)?;
    if tag != SEQUENCE {
        return None;
    }
    while !tbs.is_empty() {
        let (tag, contents, rest) = element(tbs)?;
        tbs = rest;
        if tag != EXTENSIONS {
            continue;
        }
        let (tag, mut extensions, _) = element(contents)?;
        if tag != SEQUENCE {
            return None;
        }
        while !extensions.is_empty() {
            let (tag, extension, rest) = element(extensions)?;
            extensions = rest;
            if tag != SEQUENCE {
                return None;
            }
            let (tag, oid, mut fields) = element(extension)?;
            if tag != OID || oid != BASIC_CONSTRAINTS {
                continue;
            }
            // `critical`, where stated, then the value's octets.
            let (mut tag, mut value, rest) = element(fields)?;
            if tag == BOOLEAN {
                fields = rest;
                (tag, value, _) = element(fields)?;
            }
            if tag != OCTETS {
                return None;
            }
            let (tag, constraints, _) = element(value)?;
            if tag != SEQUENCE {
                return None;
            }
            if constraints.is_empty() {
                return Some(false);
            }
            let (tag, ca, _) = element(constraints)?;
            return Some(tag == BOOLEAN && ca.first().is_some_and(|&b| b != 0));
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::{element, is_ca};

    fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        if contents.len() < 0x80 {
            out.push(u8::try_from(contents.len()).unwrap());
        } else {
            out.push(0x82);
            out.extend_from_slice(&u16::try_from(contents.len()).unwrap().to_be_bytes());
        }
        out.extend_from_slice(contents);
        out
    }

    /// A certificate's outline: what the parser reads of it, the TBS's fields before the extensions stood in for by
    /// one integer.
    fn certificate(extensions: Option<Vec<u8>>) -> Vec<u8> {
        let mut tbs = tlv(0x02, &[0x01]);
        if let Some(extensions) = extensions {
            tbs.extend(tlv(0xa3, &tlv(0x30, &extensions)));
        }
        let mut body = tlv(0x30, &tbs);
        body.extend(tlv(0x30, &[]));
        tlv(0x30, &body)
    }

    fn basic_constraints(critical: bool, ca: Option<bool>) -> Vec<u8> {
        let mut value = Vec::new();
        if let Some(ca) = ca {
            value.extend(tlv(0x01, &[if ca { 0xff } else { 0x00 }]));
        }
        let mut extension = tlv(0x06, &[0x55, 0x1d, 0x13]);
        if critical {
            extension.extend(tlv(0x01, &[0xff]));
        }
        extension.extend(tlv(0x04, &tlv(0x30, &value)));
        tlv(0x30, &extension)
    }

    // verifies: DEC-28
    #[test]
    fn only_a_certificate_whose_basic_constraints_name_it_a_ca_is_one() {
        assert!(is_ca(&certificate(Some(basic_constraints(
            true,
            Some(true)
        )))));
        assert!(is_ca(&certificate(Some(basic_constraints(
            false,
            Some(true)
        )))));
        assert!(!is_ca(&certificate(Some(basic_constraints(
            true,
            Some(false)
        )))));
        assert!(!is_ca(&certificate(Some(basic_constraints(true, None)))));
        // A leaf's: extensions, none of them basic constraints; or no extensions at all.
        let key_usage = tlv(0x30, &{
            let mut e = tlv(0x06, &[0x55, 0x1d, 0x0f]);
            e.extend(tlv(0x04, &tlv(0x03, &[0x05, 0xa0])));
            e
        });
        assert!(!is_ca(&certificate(Some(key_usage))));
        assert!(!is_ca(&certificate(None)));
        assert!(!is_ca(b"not der"));
        assert!(element(&[0x30, 0x85, 1, 1, 1, 1, 1]).is_none());
    }
}
