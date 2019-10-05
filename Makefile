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

.PHONY: all loader qemu tests clippy clean lib docs

default: lib
	make arch=$(arch) release=$(release) -C tests

all: loader lib
	make arch=$(arch) release=$(release) -C tests

clean:
	$(RM) target/x86_64-unknown-hermit-kernel
	make -C tests clean
	make -C loader clean

loader:
	make -C loader release=$(release)

qemu:
	qemu-system-x86_64 -display none -smp 1 -m 64M -serial stdio  -kernel loader/target/$(target)-loader/$(rdir)/hermit-loader -initrd tests/target/$(target)/$(rdir)/hctests -cpu qemu64,apic,fsgsbase,pku,rdtscp,xsave,fxsr

docs:
	@echo DOC
	@cargo doc --no-deps

clippy:
	@echo Run clippy...
	@RUST_TARGET_PATH=$(CURDIR) cargo clippy --target $(target)

lib:
	@echo Build libhermit
	@RUST_TARGET_PATH=$(CURDIR) cargo xbuild $(opt) --target $(target)-kernel
