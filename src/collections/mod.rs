use crate::arch::irq;

/// `irqsave` guarantees that the call of the closure
/// will be not disturbed by an interrupt
#[inline]
pub fn irqsave<F, R>(f: F) -> R
where
	F: FnOnce() -> R,
{
	let irq = irq::nested_disable();
	let ret = f();
	irq::nested_enable(irq);
	ret
}
