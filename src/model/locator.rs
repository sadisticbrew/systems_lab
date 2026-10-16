//! Locator grammar:
//!
//! ```text
//! LOC      := "step:" N | "step:end" | FILE ":" LINE [ "@" K ] | ADDRSPEC [ "@" K ]
//! ADDRSPEC := SYMBOL [ "+" OFFSET ] | "0x" HEX
//! SYMBOL   := exe symbol name (`printf@plt` included) | libfile ":" symbol name
//! OFFSET   := decimal | "0x" hex;   K := integer >= 1 (default 1)
//! ```
//!
//! When the text after the last `:` is all digits, the locator is `FILE:LINE`, not `libfile:symbol`.

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
    /// `step:end`: the final state.
    End,
    /// `FILE:LINE@K`; `text` is `FILE:LINE` as typed.
    Line { file: String, line: u32, text: String, k: u64 },
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
        if n == "end" {
            return Ok(Locator::End);
        }
        return Ok(Locator::Step(number(n).ok_or_else(bad)?));
    }
    let (text, k) = match input.rsplit_once('@') {
        // `printf@plt` names a PLT stub: the `@` belongs to the symbol.
        Some((_, tail)) if tail == "plt" || tail.starts_with("plt+") => (input, 1),
        Some((text, k)) => {
            let k = k.bytes().all(|b| b.is_ascii_digit()).then(|| k.parse::<u64>().ok()).flatten().filter(|&k| k >= 1);
            (text, k.ok_or_else(bad)?)
        }
        None => (input, 1),
    };
    if let Some((file, line)) = text.rsplit_once(':')
        && !file.is_empty()
        && !file.contains(char::is_whitespace)
        && !line.is_empty()
        && line.bytes().all(|b| b.is_ascii_digit())
    {
        return Ok(Locator::Line { file: file.to_string(), line: line.parse().map_err(|_| bad())?, text: text.to_string(), k });
    }
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
    fn step_end() {
        assert_eq!(parse("step:end").unwrap(), Locator::End);
        assert!(parse("step:End").is_err());
    }

    #[test]
    fn line_forms() {
        let line = |file: &str, line: u32, text: &str, k: u64| Locator::Line { file: file.into(), line, text: text.into(), k };
        assert_eq!(parse("fact.c:4").unwrap(), line("fact.c", 4, "fact.c:4", 1));
        assert_eq!(parse("fact.c:4@3").unwrap(), line("fact.c", 4, "fact.c:4", 3));
        assert_eq!(parse("src/fact.c:12@2").unwrap(), line("src/fact.c", 12, "src/fact.c:12", 2));
        // Anything but digits after the last colon keeps it a library symbol.
        assert_eq!(parse("libc.so.6:printf").unwrap(), addr(sym("libc.so.6:printf", 0), "libc.so.6:printf", 1));
        assert_eq!(parse("libc.so.6:printf+4").unwrap(), addr(sym("libc.so.6:printf", 4), "libc.so.6:printf+4", 1));
        assert!(parse("fact.c:99999999999").is_err());
    }

    #[test]
    fn plt_names_keep_their_at_sign() {
        assert_eq!(parse("printf@plt").unwrap(), addr(sym("printf@plt", 0), "printf@plt", 1));
        assert_eq!(parse("printf@plt@2").unwrap(), addr(sym("printf@plt", 0), "printf@plt", 2));
        assert_eq!(parse("printf@plt+0x6").unwrap(), addr(sym("printf@plt", 6), "printf@plt+0x6", 1));
        assert_eq!(parse("printf@plt+6@3").unwrap(), addr(sym("printf@plt", 6), "printf@plt+6", 3));
    }

    #[test]
    fn parse_errors() {
        for bad in ["", "step:", "step:x", "step:-1", "fact+", "fact+zz", "fact@0", "fact@", "fact@x", "0x", "0xzz", "+4", "a b", "libc.so.6:"] {
            assert_eq!(err(bad), format!("cannot parse locator '{bad}'"), "{bad}");
        }
    }
}
