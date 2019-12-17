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

.PHONY: all loader clippy clean docs

all: loader

clean:
	@cargo clean

docs:
	@echo DOC
	@cargo doc

clippy:
	@echo Run clippy...
	@RUST_TARGET_PATH=$(CURDIR) cargo clippy --target $(target)

loader:
	@echo Build loader
	@RUST_TARGET_PATH=$(CURDIR) cargo xbuild $(opt) --target $(target)-loader
	@objcopy --strip-debug -O elf32-i386 target/$(target)-loader/$(rdir)/rusty-loader
