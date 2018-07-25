pub const GICD_BASE: u64 = 1 << 39;
pub const GICC_BASE: u64 = GICD_BASE + GICD_SIZE;
pub const GIC_SIZE: u64 = GICD_SIZE + GICC_SIZE;
pub const GICD_SIZE: u64 = 0x10000;
pub const GICC_SIZE: u64 = 0x20000;

pub const GICR_BASE: u32 = 0;

/* GIC Distributor interface register offsets that are common to GICv3 & GICv2 */

pub const GICD_CTLR: u32 = 0;
pub const GICD_TYPER: u32 = 4;
pub const GICD_IIDR: u32 = 8;
pub const GICD_IGROUPR: u32 = 128;
pub const GICD_ISENABLER: u32 = 256;
pub const GICD_ICENABLER: u32 = 384;
pub const GICD_ISPENDR: u32 = 512;
pub const GICD_ICPENDR: u32 = 640;
pub const GICD_ISACTIVER: u32 = 768;
pub const GICD_ICACTIVER: u32 = 896;
pub const GICD_IPRIORITYR: u32 = 1024;
pub const GICD_ITARGETSR: u32 = 2048;
pub const GICD_ICFGR: u32 = 3072;
pub const GICD_NSACR: u32 = 3584;
pub const GICD_SGIR: u32 = 3840;

pub const GICD_CTLR_ENABLEGRP0: u32 = 1;
pub const GICD_CTLR_ENABLEGRP1: u32 = 2;

/* Physical CPU Interface registers */

pub const GICC_CTLR: u32 = 0;
pub const GICC_PMR: u32 = 4;
pub const GICC_BPR: u32 = 8;
pub const GICC_IAR: u32 = 12;
pub const GICC_EOIR: u32 = 16;
pub const GICC_RPR: u32 = 20;
pub const GICC_HPPIR: u32 = 24;
pub const GICC_AHPPIR: u32 = 40;
pub const GICC_IIDR: u32 = 252;
pub const GICC_DIR: u32 = 4096;
pub const GICC_PRIODROP: u32 = 16;

pub const GICC_CTLR_ENABLEGRP0: u32 = 1;
pub const GICC_CTLR_ENABLEGRP1: u32 = 2;
pub const GICC_CTLR_FIQEN: u32 = 8;
pub const GICC_CTLR_ACKCTL: u32 = 4;

pub struct Gicc;
pub struct Gicd;

impl Gicc {
    #[inline]
    fn read(off: usize) -> u32 {
        let mut value: u32;
        unsafe {
            asm!("ldar $w0, [$1]" : "=r"(value) : "r"(GICD_BASE as usize + off) : "memory");
        }
        return value;
    }

    #[inline]
    fn write(off: usize, value: u32) {
        unsafe {
            asm!("str $w0, [$1]" : : "rz"(value) , "r"(GICD_BASE as usize + off) : "memory");
        }
    }

    pub fn enable() {
        Gicc::write(
            GICC_CTLR as usize,
            GICC_CTLR_ENABLEGRP0 | GICC_CTLR_ENABLEGRP1 | GICC_CTLR_FIQEN | GICC_CTLR_ACKCTL,
        );
    }

    pub fn disable() {
        // Global disable signalling of interrupt from the cpu interface
        Gicc::write(GICC_CTLR as usize, 0);
    }

    pub fn set_priority(priority: u32) {
        Gicc::write(GICC_PMR as usize, priority & 0xFF);
    }
}

impl Gicd {
    #[inline]
    fn read(off: usize) -> u32 {
        let mut value: u32;
        unsafe {
            asm!("ldar $w0, [$1]" : "=r"(value) : "r"(GICC_BASE as usize + off) : "memory");
        }
        return value;
    }

    #[inline]
    fn write(off: usize, value: u32) {
        unsafe {
            asm!("str $w0, [$1]" : : "rz"(value) , "r"(GICC_BASE as usize + off) : "memory");
        }
    }

    pub fn enable() {
        // Global enable forwarding interrupts from distributor to cpu interface
        Gicd::write(
            GICD_CTLR as usize,
            GICD_CTLR_ENABLEGRP0 | GICD_CTLR_ENABLEGRP1,
        );
    }

    pub fn disable() {
        // Global disable forwarding interrupts from distributor to cpu interface
        Gicd::write(GICD_CTLR as usize, 0);
    }
}

pub fn gic_set_enable(vector: u32, enable: bool) {
    let regoff = if enable {
        GICD_ISENABLER + 4 * (vector / 32)
    } else {
        GICD_ICENABLER + 4 * (vector / 32)
    };
    Gicd::write(
        regoff as usize,
        Gicd::read(regoff as usize) | (1 << (vector % 32)),
    );
}
