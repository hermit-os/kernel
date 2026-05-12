use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};

const GDT_LEN: usize = 3;

pub(super) static GDT: GlobalDescriptorTable<GDT_LEN> = Gdt::gdt();

pub(super) static GDT_PTR: DescriptorTablePointer<'static, GDT_LEN> =
    DescriptorTablePointer::new(&GDT);

pub(super) struct Gdt;

impl Gdt {
    const fn create() -> (
        GlobalDescriptorTable<GDT_LEN>,
        SegmentSelector,
        SegmentSelector,
    ) {
        let mut gdt = GlobalDescriptorTable::empty();
        let kernel_code_selector = gdt.append(Descriptor::kernel_code_segment());
        let kernel_data_selector = gdt.append(Descriptor::kernel_data_segment());
        (gdt, kernel_code_selector, kernel_data_selector)
    }

    pub(super) const fn gdt() -> GlobalDescriptorTable<GDT_LEN> {
        Self::create().0
    }

    pub(super) const fn kernel_code_selector() -> SegmentSelector {
        Self::create().1
    }

    pub(super) const fn kernel_data_selector() -> SegmentSelector {
        Self::create().2
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed(2))]
pub(super) struct DescriptorTablePointer<'a, const MAX: usize> {
    limit: u16,
    base: &'a GlobalDescriptorTable<MAX>,
}

impl<'a, const MAX: usize> DescriptorTablePointer<'a, MAX> {
    const fn new(gdt: &'a GlobalDescriptorTable<MAX>) -> Self {
        Self {
            limit: gdt.limit(),
            base: gdt,
        }
    }
}
