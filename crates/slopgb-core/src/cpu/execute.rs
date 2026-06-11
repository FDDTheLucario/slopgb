//! Instruction decode and execution. CPU work package.

use super::{Bus, Cpu};

/// Execute one instruction (preceded by interrupt dispatch if one is
/// pending and IME is set), or one M-cycle of halt.
pub fn step(_cpu: &mut Cpu, _bus: &mut impl Bus) {
    todo!("CPU work package: full SM83 decode/execute")
}
