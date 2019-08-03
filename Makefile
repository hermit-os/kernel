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

.PHONY: all loader clippy clean lib docs

default: lib
	make arch=$(arch) release=$(release) -C examples

all: loader lib
	make arch=$(arch) release=$(release) -C examples

clean:
	$(RM) target
	make -C examples clean
	make -C loader clean

loader:
	make -C loader release=$(release)

qemu:
	qemu-system-x86_64 -display none -smp 1 -m 1G -serial stdio  -kernel loader/target/$(target)-loader/$(rdir)/hermit-loader -initrd examples/target/$(target)/$(rdir)/hctests -cpu Haswell-noTSX,vendor=GenuineIntel

docs:
	@echo DOC
	@cargo doc

clippy:
	@echo Run clippy...
	@RUST_TARGET_PATH=$(CURDIR) cargo clippy --target $(target)

lib:
	@echo Build libhermit
	@RUST_TARGET_PATH=$(CURDIR) cargo xbuild $(opt) --target $(target)-kernel
	@$(arch)-hermit-elfedit --output-osabi Standalone target/$(target)-kernel/$(rdir)/libhermit.a
