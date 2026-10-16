//! Expressions evaluated at a state (`/api/eval`, `/api/watch`, guides): registers, `stack0`, `heap`,
//! C variables, symbols, `disp(base,index,scale)` operands, `+`, `-` and `mod`; and typed reads of memory.
//!
//! ```text
//! expr   := sum [ "mod" NUMBER ]
//! sum    := term { ("+" | "-") term }
//! term   := NUMBER | ["%"] REG | "stack0" | "heap" | IDENT | memref
//! memref := [ ["-"] NUMBER ] "(" ["%"] REG [ "," ["%"] REG [ "," SCALE ] ] ")"
//! ```
//!
//! A number may be written with a leading `-`. Arithmetic wraps at 64 bits. A memref denotes the
//! address `base + index*scale + disp`, not the memory there.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use iced_x86::Register;
use serde::Serialize;

use super::dwarf::Enc;
use super::views::*;
use super::LoadedTrace;
use crate::exit;
use crate::trace::RegFile;
use crate::x86;

/// How a watched value is read and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchType {
    /// The expression's own 64-bit value.
    Value,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    /// 8 bytes shown with `render_value`.
    Ptr,
    /// NUL-terminated text.
    Str,
    /// The first 16 bytes as hex.
    Bytes,
}

impl WatchType {
    pub fn name(self) -> &'static str {
        match self {
            WatchType::Value => "value",
            WatchType::U8 => "u8",
            WatchType::I8 => "i8",
            WatchType::U16 => "u16",
            WatchType::I16 => "i16",
            WatchType::U32 => "u32",
            WatchType::I32 => "i32",
            WatchType::U64 => "u64",
            WatchType::I64 => "i64",
            WatchType::F32 => "f32",
            WatchType::F64 => "f64",
            WatchType::Ptr => "ptr",
            WatchType::Str => "str",
            WatchType::Bytes => "bytes",
        }
    }

    /// Bytes a read of the type consumes; `None` for `str` and `bytes`.
    fn width(self) -> Option<usize> {
        match self {
            WatchType::U8 | WatchType::I8 => Some(1),
            WatchType::U16 | WatchType::I16 => Some(2),
            WatchType::U32 | WatchType::I32 | WatchType::F32 => Some(4),
            WatchType::Value | WatchType::U64 | WatchType::I64 | WatchType::F64 | WatchType::Ptr => Some(8),
            WatchType::Str | WatchType::Bytes => None,
        }
    }
}

impl fmt::Display for WatchType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for WatchType {
    type Err = String;

    fn from_str(s: &str) -> Result<WatchType, String> {
        const ALL: [WatchType; 14] = [
            WatchType::Value,
            WatchType::U8,
            WatchType::I8,
            WatchType::U16,
            WatchType::I16,
            WatchType::U32,
            WatchType::I32,
            WatchType::U64,
            WatchType::I64,
            WatchType::F32,
            WatchType::F64,
            WatchType::Ptr,
            WatchType::Str,
            WatchType::Bytes,
        ];
        ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(s)).ok_or_else(|| format!("unknown type '{s}'"))
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(u64),
    /// A name: register, keyword, C variable or symbol (`printf@plt`).
    Word(String),
    /// `libfile:symbol`.
    LibSym(String),
    Plus,
    Minus,
    LParen,
    RParen,
    Comma,
    Percent,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Num(n) => write!(f, "{n}"),
            Tok::Word(w) | Tok::LibSym(w) => f.write_str(w),
            Tok::Plus => f.write_str("+"),
            Tok::Minus => f.write_str("-"),
            Tok::LParen => f.write_str("("),
            Tok::RParen => f.write_str(")"),
            Tok::Comma => f.write_str(","),
            Tok::Percent => f.write_str("%"),
        }
    }
}

fn is_word_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'.'
}

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'@' | b'$')
}

/// Splits `text` into tokens. `libs` are the file names of the loaded shared objects, so that
/// `libstdc++.so.6:foo` lexes as one symbol while `n+libc.so.6:foo` does not.
fn lex(text: &str, libs: &[&str]) -> Result<Vec<Tok>> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        let single = match c {
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
                continue;
            }
            b'+' => Some(Tok::Plus),
            b'-' => Some(Tok::Minus),
            b'(' => Some(Tok::LParen),
            b')' => Some(Tok::RParen),
            b',' => Some(Tok::Comma),
            b'%' => Some(Tok::Percent),
            _ => None,
        };
        if let Some(t) = single {
            out.push(t);
            i += 1;
        } else if c.is_ascii_digit() {
            let hex = b[i] == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X'));
            let start = if hex { i + 2 } else { i };
            let mut end = start;
            while end < b.len() && if hex { b[end].is_ascii_hexdigit() } else { b[end].is_ascii_digit() } {
                end += 1;
            }
            if end == start || b.get(end).is_some_and(|&n| n.is_ascii_alphanumeric() || n == b'_') {
                let stop = (end..b.len()).find(|&k| !(b[k].is_ascii_alphanumeric() || b[k] == b'_')).unwrap_or(b.len());
                bail!(exit::usage(format!("bad number '{}'", &text[i..stop])));
            }
            let n = u64::from_str_radix(&text[start..end], if hex { 16 } else { 10 })
                .map_err(|_| exit::usage(format!("number '{}' is too large", &text[i..end])))?;
            out.push(Tok::Num(n));
            i = end;
        } else if is_word_start(c) {
            let rest = &text[i..];
            let lib = libs.iter().filter(|l| rest.strip_prefix(**l).is_some_and(|r| r.starts_with(':'))).max_by_key(|l| l.len());
            let from = i + lib.map_or(0, |l| l.len() + 1);
            let mut end = from;
            while end < b.len() && is_word_char(b[end]) {
                end += 1;
            }
            if lib.is_some() && end > from {
                out.push(Tok::LibSym(text[i..end].to_string()));
            } else {
                let mut end = i;
                while end < b.len() && is_word_char(b[end]) {
                    end += 1;
                }
                out.push(Tok::Word(text[i..end].to_string()));
                i = end;
                continue;
            }
            i = end;
        } else {
            let ch = text[i..].chars().next().unwrap_or('?');
            bail!(exit::usage(format!("unexpected character '{ch}' in expression")));
        }
    }
    Ok(out)
}

/// General-purpose register names of every width, plus `rip`, by lowercase AT&T name.
static REGISTER_NAMES: LazyLock<HashMap<String, Register>> = LazyLock::new(|| {
    let mut m: HashMap<String, Register> = Register::values().filter(|r| r.is_gpr()).map(|r| (x86::reg_name(r), r)).collect();
    m.insert("rip".to_string(), Register::RIP);
    m
});

/// The natural type of a C variable.
fn natural_type(enc: Enc, size: u64) -> WatchType {
    match (enc, size) {
        (Enc::Signed, 1) => WatchType::I8,
        (Enc::Signed, 2) => WatchType::I16,
        (Enc::Signed, 4) => WatchType::I32,
        (Enc::Signed, 8) => WatchType::I64,
        (Enc::Unsigned, 1) => WatchType::U8,
        (Enc::Unsigned, 2) => WatchType::U16,
        (Enc::Unsigned, 4) => WatchType::U32,
        (Enc::Unsigned, 8) => WatchType::U64,
        (Enc::Bool, _) => WatchType::U8,
        (Enc::Float, 4) => WatchType::F32,
        (Enc::Float, 8) => WatchType::F64,
        (Enc::Pointer, _) => WatchType::Ptr,
        _ => WatchType::Bytes,
    }
}

/// A register that is not a general-purpose one but that expressions name.
fn special_register(name: &str, regs: &RegFile) -> Option<u64> {
    match name {
        "rflags" => Some(regs.rflags),
        "fs_base" => Some(regs.fs_base),
        "gs_base" => Some(regs.gs_base),
        _ => None,
    }
}

struct Parser<'a> {
    lt: &'a LoadedTrace,
    s: u32,
    regs: RegFile,
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += usize::from(t.is_some());
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        let hit = self.peek() == Some(t);
        self.pos += usize::from(hit);
        hit
    }

    fn expect(&mut self, t: &Tok) -> Result<()> {
        if self.eat(t) { Ok(()) } else { Err(self.unexpected(&format!("expected '{t}'"))) }
    }

    fn unexpected(&self, what: &str) -> anyhow::Error {
        match self.peek() {
            Some(t) => exit::usage(format!("{what}, found '{t}'")),
            None => exit::usage(format!("{what}, found the end of the expression")),
        }
    }

    /// `sum [ "mod" NUMBER ]`, and the type a lone term suggests.
    fn expr(&mut self) -> Result<(u64, WatchType)> {
        let (mut v, ty) = self.term()?;
        let mut compound = false;
        loop {
            let plus = match self.peek() {
                Some(Tok::Plus) => true,
                Some(Tok::Minus) => false,
                _ => break,
            };
            self.pos += 1;
            let (r, _) = self.term()?;
            v = if plus { v.wrapping_add(r) } else { v.wrapping_sub(r) };
            compound = true;
        }
        if matches!(self.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("mod")) {
            self.pos += 1;
            match self.peek().cloned() {
                Some(Tok::Num(0)) => bail!(exit::usage("mod by zero")),
                Some(Tok::Num(n)) => {
                    self.pos += 1;
                    v %= n;
                }
                _ => return Err(self.unexpected("expected a number after 'mod'")),
            }
            compound = true;
        }
        if self.peek().is_some() {
            return Err(self.unexpected("unexpected input"));
        }
        Ok((v, if compound { WatchType::Value } else { ty }))
    }

    fn term(&mut self) -> Result<(u64, WatchType)> {
        match self.next() {
            Some(Tok::Minus) => match self.peek().cloned() {
                Some(Tok::Num(n)) => {
                    self.pos += 1;
                    self.number_or_memref(n.wrapping_neg())
                }
                _ => Err(self.unexpected("expected a number after '-'")),
            },
            Some(Tok::Num(n)) => self.number_or_memref(n),
            Some(Tok::LParen) => self.memref(0),
            Some(Tok::Percent) => match self.peek().cloned() {
                Some(Tok::Word(w)) => {
                    self.pos += 1;
                    Ok((self.register(&w)?, WatchType::Value))
                }
                _ => Err(self.unexpected("expected a register name after '%'")),
            },
            Some(Tok::Word(w)) => self.word(&w),
            Some(Tok::LibSym(name)) => match self.lt.syms.lookup(&name) {
                Some((addr, _)) => Ok((addr, WatchType::Ptr)),
                None => bail!(exit::usage(format!("unknown name '{name}'"))),
            },
            Some(t) => bail!(exit::usage(format!("unexpected '{t}' in expression"))),
            None => bail!(exit::usage("the expression is incomplete")),
        }
    }

    fn number_or_memref(&mut self, n: u64) -> Result<(u64, WatchType)> {
        if self.eat(&Tok::LParen) { self.memref(n) } else { Ok((n, WatchType::Value)) }
    }

    /// The part of a memory operand after `disp(`.
    fn memref(&mut self, disp: u64) -> Result<(u64, WatchType)> {
        let base = self.reg_operand()?;
        let mut addr = disp.wrapping_add(base);
        if self.eat(&Tok::Comma) {
            let index = self.reg_operand()?;
            let scale = if self.eat(&Tok::Comma) {
                match self.next() {
                    Some(Tok::Num(n @ (1 | 2 | 4 | 8))) => n,
                    _ => bail!(exit::usage("the scale of a memory operand is 1, 2, 4 or 8")),
                }
            } else {
                1
            };
            addr = addr.wrapping_add(index.wrapping_mul(scale));
        }
        self.expect(&Tok::RParen)?;
        Ok((addr, WatchType::Ptr))
    }

    fn reg_operand(&mut self) -> Result<u64> {
        self.eat(&Tok::Percent);
        match self.peek().cloned() {
            Some(Tok::Word(w)) => {
                self.pos += 1;
                self.register(&w)
            }
            _ => Err(self.unexpected("expected a register")),
        }
    }

    fn register(&self, name: &str) -> Result<u64> {
        let lower = name.to_ascii_lowercase();
        self.register_value(&lower).ok_or_else(|| exit::usage(format!("unknown register '{name}'")))
    }

    fn register_value(&self, lower: &str) -> Option<u64> {
        if let Some(&r) = REGISTER_NAMES.get(lower) {
            return x86::reg_value(&self.regs, r);
        }
        special_register(lower, &self.regs)
    }

    /// A bare name: keyword, C variable in scope, register, or symbol.
    fn word(&mut self, w: &str) -> Result<(u64, WatchType)> {
        let lower = w.to_ascii_lowercase();
        match lower.as_str() {
            "stack0" => return Ok((self.lt.stack0, WatchType::Value)),
            "heap" => {
                return match self.lt.maps_at(self.s).regions.iter().find(|r| r.path == "[heap]") {
                    Some(r) => Ok((r.start, WatchType::Value)),
                    None => bail!(exit::usage("no heap at this step")),
                };
            }
            _ => {}
        }
        // A C variable shadows a short register name such as `dx`; `%dx` always means the register.
        if let Some((addr, v)) = self.lt.var_at(self.s, w) {
            return Ok((addr, natural_type(v.enc, v.size)));
        }
        if let Some(v) = self.register_value(&lower) {
            return Ok((v, WatchType::Value));
        }
        match self.lt.syms.lookup(w) {
            Some((addr, _)) => Ok((addr, WatchType::Ptr)),
            None => bail!(exit::usage(format!("unknown name '{w}'"))),
        }
    }
}

/// The result of reading a watched expression at one state.
struct Watched {
    addr: Option<u64>,
    value: TypedValue,
}

impl Watched {
    /// What changes when the value changes: the exact bits when there are any, else the text.
    fn key(&self) -> Option<&str> {
        self.value.raw.as_deref().or(self.value.text.as_deref())
    }
}

impl LoadedTrace {
    /// `GET /api/eval`.
    pub fn eval(&self, s: u32, expr: &str) -> Result<EvalView> {
        self.check_step(s)?;
        let (v, default_type) = self.eval_value(s, expr)?;
        Ok(EvalView { value: format!("0x{v:x}"), rendered: self.render_value(v, s), default_type })
    }

    /// The 64-bit value of `expr` in `state(s)` and the type a lone term suggests.
    fn eval_value(&self, s: u32, expr: &str) -> Result<(u64, WatchType)> {
        let libs: Vec<&str> = self.trace.libs.iter().map(|l| l.file_name.as_str()).collect();
        let toks = lex(expr, &libs)?;
        if toks.is_empty() {
            bail!(exit::usage("the expression is empty"));
        }
        Parser { lt: self, s, regs: self.regs_at(s), toks, pos: 0 }.expr()
    }

    /// `n` bytes at `addr` in `state(s)`, if all are mapped.
    fn bytes_at(&self, s: u32, addr: u64, n: usize) -> Option<Vec<u8>> {
        self.read_mem(s, addr, n).into_iter().collect()
    }

    /// Reads memory at `addr` in `state(s)` as `ty`. Integers print in decimal with their bits as
    /// `raw`; a byte that is not mapped gives `text: None`.
    pub fn read_typed(&self, s: u32, addr: u64, ty: WatchType) -> TypedValue {
        match ty {
            WatchType::Str => {
                let mut text = Vec::new();
                let mut terminated = false;
                for b in self.read_mem(s, addr, 64) {
                    match b {
                        None => return TypedValue::default(),
                        Some(0) => {
                            terminated = true;
                            break;
                        }
                        Some(b) => text.push(b),
                    }
                }
                let shown = format!("\"{}\"{}", String::from_utf8_lossy(&text).escape_debug(), if terminated { "" } else { "…" });
                TypedValue { text: Some(shown), raw: None }
            }
            WatchType::Bytes => {
                let text = self.bytes_at(s, addr, 16).map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "));
                TypedValue { text, raw: None }
            }
            _ => {
                let width = ty.width().unwrap_or(8);
                let Some(b) = self.bytes_at(s, addr, width) else { return TypedValue::default() };
                let mut le = [0u8; 8];
                le[..width].copy_from_slice(&b);
                let v = u64::from_le_bytes(le);
                let shift = 64 - 8 * width as u32;
                let signed = || (((v << shift) as i64) >> shift).to_string();
                let text = match ty {
                    WatchType::I8 | WatchType::I16 | WatchType::I32 | WatchType::I64 => signed(),
                    WatchType::U8 | WatchType::U16 | WatchType::U32 | WatchType::U64 => v.to_string(),
                    WatchType::F32 => f32::from_bits(v as u32).to_string(),
                    WatchType::F64 => f64::from_bits(v).to_string(),
                    _ => self.render_value(v, s),
                };
                TypedValue { text: Some(text), raw: Some(format!("0x{v:x}")) }
            }
        }
    }

    /// Reads the value `v` of a watched expression as `ty` in `state(s)`.
    fn watched(&self, s: u32, v: u64, ty: WatchType) -> Watched {
        if ty == WatchType::Value {
            Watched { addr: None, value: TypedValue { text: Some(self.render_value(v, s)), raw: Some(format!("0x{v:x}")) } }
        } else {
            Watched { addr: Some(v), value: self.read_typed(s, v, ty) }
        }
    }

    /// `GET /api/watch`: each expression with its value at `s` and at `s - 1`. A missing type is the
    /// one `eval` suggests; an expression that does not evaluate carries its message in `error`.
    pub fn watch(&self, s: u32, items: &[(String, Option<WatchType>)]) -> Vec<WatchView> {
        items.iter().map(|(expr, ty)| self.watch_one(s, expr, *ty)).collect()
    }

    fn watch_one(&self, s: u32, expr: &str, want: Option<WatchType>) -> WatchView {
        let (v, default) = match self.eval_value(s, expr) {
            Ok(x) => x,
            Err(e) => {
                return WatchView {
                    expr: expr.to_string(),
                    ty: want.unwrap_or(WatchType::Value),
                    addr: None,
                    value: None,
                    raw: None,
                    prev: None,
                    changed: false,
                    error: Some(format!("{e:#}")),
                };
            }
        };
        let ty = want.unwrap_or(default);
        let now = self.watched(s, v, ty);
        let prev = if s > 0 { self.eval_value(s - 1, expr).ok().map(|(pv, _)| self.watched(s - 1, pv, ty)) } else { None };
        let changed = s > 0 && prev.as_ref().and_then(Watched::key) != now.key();
        WatchView {
            expr: expr.to_string(),
            ty,
            addr: now.addr.map(|a| format!("0x{a:x}")),
            value: now.value.text,
            raw: now.value.raw,
            prev: prev.and_then(|p| p.value.text),
            changed,
            error: None,
        }
    }

    /// `GET /api/inspect`: what is at `addr` in `state(s)`.
    pub fn inspect(&self, s: u32, addr: u64) -> Result<InspectView> {
        self.check_step(s)?;
        let region = match Self::region_at(self.maps_at(s), addr) {
            None => "unmapped".to_string(),
            Some(r) => {
                let name = if r.path.is_empty() {
                    "anonymous".to_string()
                } else if r.path.starts_with('[') {
                    r.path.clone()
                } else {
                    std::path::Path::new(&r.path).file_name().map_or_else(|| r.path.clone(), |n| n.to_string_lossy().into_owned())
                };
                format!("{name} ({})", r.perms)
            }
        };
        const TYPES: [WatchType; 11] = [
            WatchType::U8,
            WatchType::I8,
            WatchType::U16,
            WatchType::I16,
            WatchType::U32,
            WatchType::I32,
            WatchType::U64,
            WatchType::I64,
            WatchType::F32,
            WatchType::F64,
            WatchType::Ptr,
        ];
        let writer = self.last_writer(s, addr);
        let cstring = self.read_typed(s, addr, WatchType::Str).text.filter(|t| t != "\"\"");
        Ok(InspectView {
            addr: format!("0x{addr:x}"),
            rendered: self.render_value(addr, s),
            region,
            bytes: self.read_mem(s, addr, 8),
            values: TYPES.iter().map(|&ty| InspectValue { ty, value: self.read_typed(s, addr, ty).text }).collect(),
            cstring,
            writer,
            writer_asm: writer.map(|w| self.asm_at(w)),
            next_writer: self.nav(s, &super::NavOp::WriteNext(addr))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(text: &str) -> Vec<Tok> {
        lex(text, &["libc.so.6", "libstdc++.so.6"]).unwrap()
    }

    #[test]
    fn lexes_numbers_names_and_operators() {
        assert_eq!(toks("rbp-8"), [Tok::Word("rbp".into()), Tok::Minus, Tok::Num(8)]);
        assert_eq!(
            toks("-0x8(%rbp)"),
            [Tok::Minus, Tok::Num(8), Tok::LParen, Tok::Percent, Tok::Word("rbp".into()), Tok::RParen]
        );
        assert_eq!(toks("rsp mod 16"), [Tok::Word("rsp".into()), Tok::Word("mod".into()), Tok::Num(16)]);
        assert_eq!(toks("printf@plt+0x4"), [Tok::Word("printf@plt".into()), Tok::Plus, Tok::Num(4)]);
    }

    #[test]
    fn a_library_prefix_is_matched_against_loaded_files() {
        assert_eq!(toks("libc.so.6:printf+4"), [Tok::LibSym("libc.so.6:printf".into()), Tok::Plus, Tok::Num(4)]);
        assert_eq!(toks("libstdc++.so.6:foo"), [Tok::LibSym("libstdc++.so.6:foo".into())]);
        assert_eq!(toks("n+libc.so.6:foo"), [Tok::Word("n".into()), Tok::Plus, Tok::LibSym("libc.so.6:foo".into())]);
    }

    #[test]
    fn rejects_malformed_text() {
        for bad in ["12ab", "0x", "0xé", "7é", "$5", "99999999999999999999", "é"] {
            assert!(lex(bad, &[]).is_err(), "{bad}");
        }
    }

    #[test]
    fn watch_types_parse_case_insensitively() {
        assert_eq!("I64".parse(), Ok(WatchType::I64));
        assert_eq!("bytes".parse(), Ok(WatchType::Bytes));
        assert_eq!("long".parse::<WatchType>(), Err("unknown type 'long'".to_string()));
        assert_eq!(serde_json::to_string(&WatchType::F32).unwrap(), "\"f32\"");
    }
}
