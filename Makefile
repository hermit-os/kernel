arch ?= x86_64
target ?= $(arch)-unknown-hermit
release ?= 0

opt :=
rdir := debug

ifeq ($(release), 1)
opt := --release
rdir := release
endif

RN :=
ifdef COMSPEC
RM := del
else
RM := rm -rf
endif

GRUBCFG="default=0\ntimeout=0\n\nmenuentry \"rusty\" {\n\tmultiboot /boot/loader\n\tmodule /boot/rusty\n\tboot\n}"

.PHONY: all loader qemu tests clippy clean lib docs

default: loader lib
	make arch=$(arch) release=$(release) -C tests

all: loader lib
	make arch=$(arch) release=$(release) -C tests

clean:
	@$(RM) target *.iso
	make -C tests clean

loader:
	make -C loader release=$(release)

iso: all
	@$(RM) isodir
	@mkdir -p isodir/boot/grub
	@echo $(GRUBCFG) >> isodir/boot/grub/grub.cfg
	@cp loader/target/$(target)-loader/$(rdir)/hermit-loader isodir/boot/loader
	@cp tests/target/$(target)/$(rdir)/rusty_tests isodir/boot/rusty
	@grub-mkrescue -o test.iso isodir
	qemu-system-x86_64 -display none -smp 1 -m 64M -serial stdio -cdrom test.iso -cpu qemu64,apic,fsgsbase,pku,rdtscp,xsave,fxsr

qemu: all
	qemu-system-x86_64 -display none -smp 1 -m 64M -serial stdio  -kernel loader/target/$(target)-loader/$(rdir)/hermit-loader -initrd tests/target/$(target)/$(rdir)/rusty_tests -cpu qemu64,apic,fsgsbase,pku,rdtscp,xsave,fxsr

docs:
	@echo DOC
	@cargo doc --no-deps

clippy:
	@echo Run clippy...
	@RUST_TARGET_PATH=$(CURDIR) cargo clippy --target $(target)

lib:
	@echo Build libhermit
	@RUST_TARGET_PATH=$(CURDIR) cargo xbuild $(opt) --target $(target)-kernel
