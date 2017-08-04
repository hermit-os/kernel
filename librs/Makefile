# derived from http://blog.phil-opp.com/rust-os/multiboot-kernel.html

arch ?= x86_64
target ?= $(arch)-hermit
rust_os := target/$(target)/release/libhermit_rs.a

.PHONY: all lib default clean cargo

default: lib

lib: cargo $(rust_os)

all: runtime  lib

clean:
	@cargo clean --target $(target)

cargo:
	@echo CARGO
	@cargo build --target $(target) --release

#==========================================================================
# Building the Rust runtime for our bare-metal target

# Where to put our compiled runtime libraries for this platform.
installed_target_libs := \
	$(shell rustup which rustc | \
		sed s,bin/rustc,lib/rustlib/$(target)/lib,)

runtime_rlibs := \
	$(installed_target_libs)/libcore.rlib \
	$(installed_target_libs)/libstd_unicode.rlib \
	$(installed_target_libs)/liballoc.rlib \
	$(installed_target_libs)/libcollections.rlib

RUSTC := \
	rustc --verbose --target $(target) \
		-Z no-landing-pads \
		--out-dir $(installed_target_libs)

.PHONY: runtime

runtime: $(runtime_rlibs)

$(installed_target_libs):
	@mkdir -p $(installed_target_libs)

$(installed_target_libs)/%.rlib: rust/src/%/lib.rs $(installed_target_libs)
	@echo RUSTC $<
	@$(RUSTC) $<
	@echo Check $(installed_target_libs)
