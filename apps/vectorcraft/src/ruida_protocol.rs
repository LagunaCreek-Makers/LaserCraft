//! Offline, non-authoritative Ruida datagram inspection.
//! No controller command bytes are assumed or transmitted.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub length: usize,
    pub first_byte: Option<u8>,
    pub first_two_be: Option<u16>,
    pub byte_sum_mod_256: u8,
    pub byte_xor: u8,
    pub trailing_two_be: Option<u16>,
    pub warnings: Vec<String>,
}

pub fn inspect(bytes: &[u8]) -> Inspection {
    let mut warnings = vec!["Heuristic byte statistics only: not a verified Ruida frame or status decoder".into()];
    if bytes.is_empty() { warnings.push("Empty datagram".into()); }
    if bytes.len() > 4096 { warnings.push("Large datagram; inspect origin before treating it as controller traffic".into()); }
    Inspection {
        length: bytes.len(),
        first_byte: bytes.first().copied(),
        first_two_be: bytes.get(..2).map(|b|u16::from_be_bytes([b[0],b[1]])),
        byte_sum_mod_256: bytes.iter().fold(0u8,|sum,b|sum.wrapping_add(*b)),
        byte_xor: bytes.iter().fold(0u8,|acc,b|acc^b),
        trailing_two_be: bytes.get(bytes.len().saturating_sub(2)..).filter(|b|b.len()==2).map(|b|u16::from_be_bytes([b[0],b[1]])),
        warnings,
    }
}

pub fn parse_hex(input:&str)->Result<Vec<u8>,String>{
    let mut out=Vec::new();
    for token in input.split(|c:char|c.is_ascii_whitespace() || c==',' || c==':' || c=='-') {
        if token.is_empty(){continue;}
        let token=token.strip_prefix("0x").or_else(||token.strip_prefix("0X")).unwrap_or(token);
        if token.len()!=2 {return Err(format!("Expected two hex digits per byte: {token}"));}
        out.push(u8::from_str_radix(token,16).map_err(|_|format!("Invalid hex byte: {token}"))?);
        if out.len()>65535 {return Err("Datagram exceeds 65535 bytes".into());}
    }
    Ok(out)
}

#[derive(Debug,Default)]
pub struct CaptureAnalysis {pub by_first_byte:BTreeMap<u8,usize>,pub empty:usize,pub lengths:BTreeMap<usize,usize>}
impl CaptureAnalysis {
    pub fn add(&mut self, bytes:&[u8]) {
        *self.lengths.entry(bytes.len()).or_default()+=1;
        if let Some(first)=bytes.first(){*self.by_first_byte.entry(*first).or_default()+=1;} else {self.empty+=1;}
    }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn parses_hex_and_rejects_invalid(){assert_eq!(parse_hex("0x01 AA:ff,10").unwrap(),vec![1,170,255,16]);assert!(parse_hex("XYZ").is_err());assert!(parse_hex("1").is_err());}
 #[test] fn computes_descriptive_values(){let p=inspect(&[0x12,0x34,0x56]);assert_eq!(p.first_two_be,Some(0x1234));assert_eq!(p.byte_sum_mod_256,0x9c);assert_eq!(p.byte_xor,0x70);assert_eq!(p.trailing_two_be,Some(0x3456));}
 #[test] fn empty_is_safe(){let p=inspect(&[]);assert_eq!(p.first_byte,None);assert!(!p.warnings.is_empty());}
 #[test] fn groups_packet_shapes(){let mut a=CaptureAnalysis::default();a.add(&[0x10,0]);a.add(&[0x10,1]);a.add(&[]);assert_eq!(a.by_first_byte[&0x10],2);assert_eq!(a.lengths[&2],2);assert_eq!(a.empty,1);}
}
