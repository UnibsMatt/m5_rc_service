//! Minimal X.509 inspection (subject CN + notAfter) without a full ASN.1 dependency.

use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

use crate::util::{days_from_civil, now_unix};

#[derive(Debug, Clone, serde::Serialize)]
pub struct CertInfo {
    pub cn: Option<String>,
    pub not_after: Option<i64>,
    pub days_left: Option<i64>,
}

/// Read the first certificate of a PEM file and extract CN + expiry.
pub fn inspect_file(path: &str) -> Option<CertInfo> {
    let der = CertificateDer::pem_file_iter(path).ok()?.next()?.ok()?;
    inspect(&der)
}

pub fn inspect(der: &[u8]) -> Option<CertInfo> {
    let (_, cert, _) = tlv(der)?; // Certificate SEQUENCE
    let (_, mut tbs, _) = tlv(cert)?; // tbsCertificate SEQUENCE

    let mut next = || -> Option<(u8, &[u8])> {
        let (tag, val, rest) = tlv(tbs)?;
        tbs = rest;
        Some((tag, val))
    };
    let (mut tag, _) = next()?;
    if tag == 0xA0 {
        // [0] version present -> next is serial
        (tag, _) = next()?;
    }
    if tag != 0x02 {
        return None; // serial INTEGER expected
    }
    let _sig_alg = next()?;
    let _issuer = next()?;
    let (_, validity) = next()?;
    let (_, subject) = next()?;

    let not_after = {
        let (_, _, rest) = tlv(validity)?; // skip notBefore
        let (t, v, _) = tlv(rest)?;
        parse_time(t, v)
    };

    Some(CertInfo {
        cn: find_cn(subject),
        not_after,
        days_left: not_after.map(|na| (na - now_unix()).div_euclid(86_400)),
    })
}

/// Parse one DER TLV: returns (tag, value, remaining bytes).
fn tlv(buf: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *buf.first()?;
    let first = *buf.get(1)? as usize;
    let (len, hdr) = if first < 0x80 {
        (first, 2)
    } else {
        let nb = first & 0x7F;
        if nb == 0 || nb > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..nb {
            len = (len << 8) | *buf.get(2 + i)? as usize;
        }
        (len, 2 + nb)
    };
    let end = hdr.checked_add(len)?;
    Some((tag, buf.get(hdr..end)?, &buf[end..]))
}

fn find_cn(mut name: &[u8]) -> Option<String> {
    const OID_CN: &[u8] = &[0x55, 0x04, 0x03];
    while let Some((_, set, rest)) = tlv(name) {
        name = rest;
        let mut atvs = set;
        while let Some((_, atv, rest2)) = tlv(atvs) {
            atvs = rest2;
            let (otag, oid, after) = tlv(atv)?;
            if otag == 0x06 && oid == OID_CN {
                let (_, val, _) = tlv(after)?;
                return Some(String::from_utf8_lossy(val).into_owned());
            }
        }
    }
    None
}

/// UTCTime (0x17, YYMMDDHHMMSSZ) or GeneralizedTime (0x18, YYYYMMDDHHMMSSZ).
fn parse_time(tag: u8, v: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(v).ok()?.strip_suffix('Z')?;
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (year, off) = match tag {
        0x17 => {
            let yy = num(0, 2)?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, 2)
        }
        0x18 => (num(0, 4)?, 4),
        _ => return None,
    };
    let mo = num(off, off + 2)? as u32;
    let d = num(off + 2, off + 4)? as u32;
    let h = num(off + 4, off + 6)?;
    let mi = num(off + 6, off + 8)?;
    let se = num(off + 8, off + 10)?;
    Some(days_from_civil(year, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(parse_time(0x17, b"270205134331Z"), Some(1_801_835_011));
        assert_eq!(parse_time(0x18, b"20270205134331Z"), Some(1_801_835_011));
        assert_eq!(parse_time(0x17, b"garbage"), None);
    }

    #[test]
    fn truncated_input_is_safe() {
        assert!(inspect(&[0x30, 0x82, 0xFF]).is_none());
        assert!(inspect(&[]).is_none());
    }
}
