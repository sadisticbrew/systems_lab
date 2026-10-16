//! x86-64 register helpers shared by the recorder and the analysis model.

use iced_x86::Register;

use crate::trace::{RegFile, reg};

/// General-purpose registers in trace register-id order (`reg::RAX` …).
const GPR64: [Register; 16] = [
    Register::RAX,
    Register::RBX,
    Register::RCX,
    Register::RDX,
    Register::RSI,
    Register::RDI,
    Register::RBP,
    Register::RSP,
    Register::R8,
    Register::R9,
    Register::R10,
    Register::R11,
    Register::R12,
    Register::R13,
    Register::R14,
    Register::R15,
];

/// The value of `reg` (any width, `rip`, segment bases) in `r`; `None` for registers the trace does not record.
pub fn reg_value(r: &RegFile, reg: Register) -> Option<u64> {
    let idx = match reg.full_register() {
        Register::RAX => reg::RAX,
        Register::RBX => reg::RBX,
        Register::RCX => reg::RCX,
        Register::RDX => reg::RDX,
        Register::RSI => reg::RSI,
        Register::RDI => reg::RDI,
        Register::RBP => reg::RBP,
        Register::RSP => reg::RSP,
        Register::R8 => 8,
        Register::R9 => 9,
        Register::R10 => 10,
        Register::R11 => 11,
        Register::R12 => reg::R12,
        Register::R13 => reg::R13,
        Register::R14 => reg::R14,
        Register::R15 => reg::R15,
        Register::RIP => return Some(r.rip),
        Register::FS => return Some(r.fs_base),
        Register::GS => return Some(r.gs_base),
        Register::CS | Register::DS | Register::ES | Register::SS => return Some(0),
        _ => return None,
    };
    let v = r.gpr[idx as usize];
    Some(if reg.is_gpr64() {
        v
    } else if reg.is_gpr32() {
        v as u32 as u64
    } else if reg.is_gpr16() {
        v as u16 as u64
    } else if matches!(reg, Register::AH | Register::CH | Register::DH | Register::BH) {
        (v >> 8) & 0xff
    } else {
        v & 0xff
    })
}

/// Lowercase AT&T-style name of `r` (`rax`, `eax`, `ah`, `r9b`, `xmm3`).
pub fn reg_name(r: Register) -> String {
    let name = format!("{r:?}").to_lowercase();
    // iced calls the low byte of r8..r15 `R8L`; AT&T and the manuals say `r8b`.
    match name.strip_prefix('r').and_then(|n| n.strip_suffix('l')) {
        Some(n) if n.parse::<u8>().is_ok_and(|n| (8..=15).contains(&n)) => format!("r{n}b"),
        _ => name,
    }
}

/// The trace register id of the full register containing `r`: a general-purpose register of any width
/// (0..=15), or `xmm`/`ymm`/`zmm` 0..15 (`reg::XMM0 + n`). `None` for everything else.
pub fn full_reg_id(r: Register) -> Option<u8> {
    if r.is_xmm() || r.is_ymm() || r.is_zmm() {
        let n = r.number();
        return (n < 16).then(|| reg::XMM0 + n as u8);
    }
    if !r.is_gpr() {
        return None;
    }
    let full = r.full_register();
    GPR64.iter().position(|&g| g == full).map(|i| i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lowercase_att_names() {
        assert_eq!(reg_name(Register::RAX), "rax");
        assert_eq!(reg_name(Register::EAX), "eax");
        assert_eq!(reg_name(Register::AH), "ah");
        assert_eq!(reg_name(Register::SIL), "sil");
        assert_eq!(reg_name(Register::R8L), "r8b");
        assert_eq!(reg_name(Register::R15L), "r15b");
        assert_eq!(reg_name(Register::R9D), "r9d");
        assert_eq!(reg_name(Register::R10W), "r10w");
        assert_eq!(reg_name(Register::XMM3), "xmm3");
        assert_eq!(reg_name(Register::RIP), "rip");
        assert_eq!(reg_name(Register::FS), "fs");
    }

    #[test]
    fn full_register_ids_follow_the_trace_numbering() {
        assert_eq!(full_reg_id(Register::RAX), Some(reg::RAX));
        assert_eq!(full_reg_id(Register::EAX), Some(reg::RAX));
        assert_eq!(full_reg_id(Register::AH), Some(reg::RAX));
        assert_eq!(full_reg_id(Register::BL), Some(reg::RBX));
        assert_eq!(full_reg_id(Register::SIL), Some(reg::RSI));
        assert_eq!(full_reg_id(Register::DI), Some(reg::RDI));
        assert_eq!(full_reg_id(Register::RSP), Some(reg::RSP));
        assert_eq!(full_reg_id(Register::R8L), Some(8));
        assert_eq!(full_reg_id(Register::R11D), Some(11));
        assert_eq!(full_reg_id(Register::R15), Some(reg::R15));
        assert_eq!(full_reg_id(Register::XMM0), Some(reg::XMM0));
        assert_eq!(full_reg_id(Register::YMM15), Some(reg::XMM0 + 15));
        assert_eq!(full_reg_id(Register::ZMM7), Some(reg::XMM0 + 7));
        assert_eq!(full_reg_id(Register::XMM16), None);
        assert_eq!(full_reg_id(Register::ZMM31), None);
        for none in [Register::RIP, Register::FS, Register::CS, Register::ST0, Register::None] {
            assert_eq!(full_reg_id(none), None, "{none:?}");
        }
    }

    #[test]
    fn reg_value_reads_each_width() {
        let mut r = RegFile::default();
        r.gpr[reg::RAX as usize] = 0x1122_3344_5566_7788;
        r.gpr[9] = 0xffee_ddcc_bbaa_9988;
        assert_eq!(reg_value(&r, Register::RAX), Some(0x1122_3344_5566_7788));
        assert_eq!(reg_value(&r, Register::EAX), Some(0x5566_7788));
        assert_eq!(reg_value(&r, Register::AX), Some(0x7788));
        assert_eq!(reg_value(&r, Register::AH), Some(0x77));
        assert_eq!(reg_value(&r, Register::AL), Some(0x88));
        assert_eq!(reg_value(&r, Register::R9L), Some(0x88));
        assert_eq!(reg_value(&r, Register::XMM0), None);
    }
}
