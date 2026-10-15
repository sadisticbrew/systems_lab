//! Locator grammar:
//!
//! ```text
//! LOC      := "step:" N | ADDRSPEC [ "@" K ]
//! ADDRSPEC := SYMBOL [ "+" OFFSET ] | "0x" HEX
//! SYMBOL   := exe symbol name | libfile ":" symbol name
//! OFFSET   := decimal | "0x" hex;   K := integer >= 1 (default 1)
//! ```

use anyhow::Result;

use crate::exit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddrSpec {
    Hex(u64),
    Symbol { name: String, offset: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    Step(u64),
    /// `text` is the address spec as typed, without `@K`.
    Addr { spec: AddrSpec, text: String, k: u64 },
}

fn number(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x") {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(hex, 16).ok()
    } else if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().ok()
    } else {
        None
    }
}

pub fn parse(input: &str) -> Result<Locator> {
    let bad = || exit::usage(format!("cannot parse locator '{input}'"));
    if let Some(n) = input.strip_prefix("step:") {
        return Ok(Locator::Step(number(n).ok_or_else(bad)?));
    }
    let (text, k) = match input.rsplit_once('@') {
        Some((text, k)) => {
            let k = k.bytes().all(|b| b.is_ascii_digit()).then(|| k.parse::<u64>().ok()).flatten().filter(|&k| k >= 1);
            (text, k.ok_or_else(bad)?)
        }
        None => (input, 1),
    };
    let spec = if text.starts_with("0x") {
        AddrSpec::Hex(number(text).ok_or_else(bad)?)
    } else {
        let (name, offset) = match text.rsplit_once('+') {
            Some((name, off)) => (name, number(off).ok_or_else(bad)?),
            None => (text, 0),
        };
        if name.is_empty() || name.contains(char::is_whitespace) || name.ends_with(':') || name.starts_with(':') {
            return Err(bad());
        }
        AddrSpec::Symbol { name: name.to_string(), offset }
    };
    Ok(Locator::Addr { spec, text: text.to_string(), k })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(s: &str) -> String {
        parse(s).unwrap_err().to_string()
    }

    fn addr(spec: AddrSpec, text: &str, k: u64) -> Locator {
        Locator::Addr { spec, text: text.to_string(), k }
    }

    fn sym(name: &str, offset: u64) -> AddrSpec {
        AddrSpec::Symbol { name: name.to_string(), offset }
    }

    #[test]
    fn step() {
        assert_eq!(parse("step:0").unwrap(), Locator::Step(0));
        assert_eq!(parse("step:63").unwrap(), Locator::Step(63));
        assert_eq!(parse("step:0x10").unwrap(), Locator::Step(16));
    }

    #[test]
    fn symbol_forms() {
        assert_eq!(parse("fact").unwrap(), addr(sym("fact", 0), "fact", 1));
        assert_eq!(parse("fact+12").unwrap(), addr(sym("fact", 12), "fact+12", 1));
        assert_eq!(parse("fact+0xc").unwrap(), addr(sym("fact", 12), "fact+0xc", 1));
        assert_eq!(parse("fact+0xc@3").unwrap(), addr(sym("fact", 12), "fact+0xc", 3));
        assert_eq!(parse("main@2").unwrap(), addr(sym("main", 0), "main", 2));
    }

    #[test]
    fn lib_symbol() {
        assert_eq!(parse("libc.so.6:puts+4@2").unwrap(), addr(sym("libc.so.6:puts", 4), "libc.so.6:puts+4", 2));
    }

    #[test]
    fn hex_address() {
        assert_eq!(parse("0x401136").unwrap(), addr(AddrSpec::Hex(0x401136), "0x401136", 1));
        assert_eq!(parse("0x401136@4").unwrap(), addr(AddrSpec::Hex(0x401136), "0x401136", 4));
    }

    #[test]
    fn parse_errors() {
        for bad in ["", "step:", "step:x", "step:-1", "fact+", "fact+zz", "fact@0", "fact@", "fact@x", "0x", "0xzz", "+4", "a b", "libc.so.6:"] {
            assert_eq!(err(bad), format!("cannot parse locator '{bad}'"), "{bad}");
        }
    }
}
