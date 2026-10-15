//! Instruction decoding and AT&T text via iced, with symbols rendered as `loc()` strings.

use std::sync::Arc;

use iced_x86::{
    Code, Decoder, DecoderOptions, Formatter, FormatterOutput, FormatterTextKind, GasFormatter, Instruction, Mnemonic, OpKind,
    SymbolResolver, SymbolResult,
};

use super::symtab::SymTab;

/// What the analysis needs to know about an instruction without formatting it.
#[derive(Debug, Clone, Default)]
pub struct InsnInfo {
    /// Near call.
    pub call: bool,
    /// Near ret.
    pub ret: bool,
    /// `push %reg`: the lowercase register name.
    pub push_reg: Option<String>,
}

pub fn decode(bytes: &[u8], addr: u64) -> Option<Instruction> {
    if bytes.is_empty() {
        return None;
    }
    let mut d = Decoder::with_ip(64, bytes, addr, DecoderOptions::NONE);
    let ins = d.decode();
    (!ins.is_invalid()).then_some(ins)
}

pub fn analyze(bytes: &[u8], addr: u64) -> InsnInfo {
    let Some(ins) = decode(bytes, addr) else { return InsnInfo::default() };
    let call = matches!(
        ins.code(),
        Code::Call_rel16 | Code::Call_rel32_32 | Code::Call_rel32_64 | Code::Call_rm16 | Code::Call_rm32 | Code::Call_rm64
    );
    let ret = ins.mnemonic() == Mnemonic::Ret;
    let push_reg = (ins.mnemonic() == Mnemonic::Push && ins.op0_kind() == OpKind::Register)
        .then(|| format!("{:?}", ins.op0_register()).to_lowercase());
    InsnInfo { call, ret, push_reg }
}

struct Resolver(Arc<SymTab>);

impl SymbolResolver for Resolver {
    fn symbol(
        &mut self,
        _instruction: &Instruction,
        _operand: u32,
        _instruction_operand: Option<u32>,
        address: u64,
        _address_size: u32,
    ) -> Option<SymbolResult<'_>> {
        self.0.in_elf(address).then(|| SymbolResult::with_string(address, self.0.loc(address)))
    }
}

struct Out(String);

impl FormatterOutput for Out {
    fn write(&mut self, text: &str, _kind: FormatterTextKind) {
        self.0.push_str(text);
    }
}

/// The two formatters: plain AT&T text and the size-suffixed mnemonic.
pub struct Fmt {
    asm: GasFormatter,
    mnemonic: GasFormatter,
}

impl Fmt {
    pub fn new(syms: &Arc<SymTab>) -> Fmt {
        let mut asm = GasFormatter::with_options(Some(Box::new(Resolver(syms.clone()))), None);
        asm.options_mut().set_small_hex_numbers_in_decimal(false);
        let mut mnemonic = GasFormatter::new();
        mnemonic.options_mut().set_gas_show_mnemonic_size_suffix(true);
        Fmt { asm, mnemonic }
    }

    pub fn asm(&mut self, ins: &Instruction) -> String {
        let mut out = Out(String::new());
        self.asm.format(ins, &mut out);
        out.0
    }

    /// The mnemonic with its AT&T size suffix, e.g. `imulq`.
    pub fn mnemonic(&mut self, ins: &Instruction) -> String {
        let mut out = Out(String::new());
        self.mnemonic.format_mnemonic(ins, &mut out);
        out.0.trim().to_string()
    }
}
