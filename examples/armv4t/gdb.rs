use core::convert::TryInto;

use armv4t_emu::{reg, Memory};
use gdbstub::arch;
use gdbstub::arch::arm::reg::id::ArmCoreRegId;
use gdbstub::target::ext::breakpoint::WatchKind;
use gdbstub::target::{base, ext, Target};

use base::singlethread::{ResumeAction, SingleThreadOps, StopReason};

use crate::emu::{Emu, Event};

// `extended_mode` extensions
mod extended_mode;

/// Turn a `ArmCoreRegId` into an internal register number of `armv4t_emu`.
fn cpu_reg_id(id: ArmCoreRegId) -> Option<u8> {
    match id {
        ArmCoreRegId::Gpr(i) => Some(i),
        ArmCoreRegId::Sp => Some(reg::SP),
        ArmCoreRegId::Lr => Some(reg::LR),
        ArmCoreRegId::Pc => Some(reg::PC),
        ArmCoreRegId::Cpsr => Some(reg::CPSR),
        _ => None,
    }
}

impl Target for Emu {
    type Arch = arch::arm::Armv4t;
    type Error = &'static str;

    fn base_ops(&mut self) -> base::BaseOps<Self::Arch, Self::Error> {
        base::BaseOps::SingleThread(self)
    }

    fn sw_breakpoint(&mut self) -> Option<ext::SwBreakpointOps<Self>> {
        Some(self)
    }

    fn hw_watchpoint(&mut self) -> Option<ext::HwWatchpointOps<Self>> {
        Some(self)
    }

    fn extended_mode(&mut self) -> Option<ext::ExtendedModeOps<Self>> {
        Some(self)
    }
}

impl SingleThreadOps for Emu {
    fn resume(
        &mut self,
        action: ResumeAction,
        check_gdb_interrupt: &mut dyn FnMut() -> bool,
    ) -> Result<StopReason<u32>, Self::Error> {
        let event = match action {
            ResumeAction::Step => match self.step() {
                Some(e) => e,
                None => return Ok(StopReason::DoneStep),
            },
            ResumeAction::Continue => {
                let mut cycles = 0;
                loop {
                    if let Some(event) = self.step() {
                        break event;
                    };

                    // check for GDB interrupt every 1024 instructions
                    cycles += 1;
                    if cycles % 1024 == 0 && check_gdb_interrupt() {
                        return Ok(StopReason::GdbInterrupt);
                    }
                }
            }
        };

        Ok(match event {
            Event::Halted => StopReason::Halted,
            Event::Break => StopReason::HwBreak,
            Event::WatchWrite(addr) => StopReason::Watch {
                kind: WatchKind::Write,
                addr,
            },
            Event::WatchRead(addr) => StopReason::Watch {
                kind: WatchKind::Read,
                addr,
            },
        })
    }

    fn read_registers(
        &mut self,
        regs: &mut arch::arm::reg::ArmCoreRegs,
    ) -> Result<(), &'static str> {
        let mode = self.cpu.mode();

        for i in 0..13 {
            regs.r[i] = self.cpu.reg_get(mode, i as u8);
        }
        regs.sp = self.cpu.reg_get(mode, reg::SP);
        regs.lr = self.cpu.reg_get(mode, reg::LR);
        regs.pc = self.cpu.reg_get(mode, reg::PC);
        regs.cpsr = self.cpu.reg_get(mode, reg::CPSR);

        Ok(())
    }

    fn write_registers(&mut self, regs: &arch::arm::reg::ArmCoreRegs) -> Result<(), &'static str> {
        let mode = self.cpu.mode();

        for i in 0..13 {
            self.cpu.reg_set(mode, i, regs.r[i as usize]);
        }
        self.cpu.reg_set(mode, reg::SP, regs.sp);
        self.cpu.reg_set(mode, reg::LR, regs.lr);
        self.cpu.reg_set(mode, reg::PC, regs.pc);
        self.cpu.reg_set(mode, reg::CPSR, regs.cpsr);

        Ok(())
    }

    fn read_register(
        &mut self,
        reg_id: arch::arm::reg::id::ArmCoreRegId,
        dst: &mut [u8],
    ) -> Result<bool, Self::Error> {
        if let Some(i) = cpu_reg_id(reg_id) {
            let w = self.cpu.reg_get(self.cpu.mode(), i);
            dst.copy_from_slice(&w.to_le_bytes());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn write_register(
        &mut self,
        reg_id: arch::arm::reg::id::ArmCoreRegId,
        val: &[u8],
    ) -> Result<bool, Self::Error> {
        let w = u32::from_le_bytes(val.try_into().map_err(|_| "invalid data")?);
        if let Some(i) = cpu_reg_id(reg_id) {
            self.cpu.reg_set(self.cpu.mode(), i, w);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn read_addrs(&mut self, start_addr: u32, data: &mut [u8]) -> Result<bool, &'static str> {
        for (addr, val) in (start_addr..).zip(data.iter_mut()) {
            *val = self.mem.r8(addr)
        }
        Ok(true)
    }

    fn write_addrs(&mut self, start_addr: u32, data: &[u8]) -> Result<bool, &'static str> {
        for (addr, val) in (start_addr..).zip(data.iter().copied()) {
            self.mem.w8(addr, val)
        }
        Ok(true)
    }
}

impl ext::breakpoint::SwBreakpoint for Emu {
    fn add_sw_breakpoint(&mut self, addr: u32) -> Result<bool, &'static str> {
        self.breakpoints.push(addr);
        Ok(true)
    }

    fn remove_sw_breakpoint(&mut self, addr: u32) -> Result<bool, &'static str> {
        match self.breakpoints.iter().position(|x| *x == addr) {
            None => return Ok(false),
            Some(pos) => self.breakpoints.remove(pos),
        };

        Ok(true)
    }
}

impl ext::breakpoint::HwWatchpoint for Emu {
    fn add_hw_watchpoint(&mut self, addr: u32, kind: WatchKind) -> Result<bool, &'static str> {
        match kind {
            WatchKind::Write => self.watchpoints.push(addr),
            WatchKind::Read => self.watchpoints.push(addr),
            WatchKind::ReadWrite => self.watchpoints.push(addr),
        };

        Ok(true)
    }

    fn remove_hw_watchpoint(&mut self, addr: u32, kind: WatchKind) -> Result<bool, &'static str> {
        let pos = match self.watchpoints.iter().position(|x| *x == addr) {
            None => return Ok(false),
            Some(pos) => pos,
        };

        match kind {
            WatchKind::Write => self.watchpoints.remove(pos),
            WatchKind::Read => self.watchpoints.remove(pos),
            WatchKind::ReadWrite => self.watchpoints.remove(pos),
        };

        Ok(true)
    }
}
