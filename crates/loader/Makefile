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

.PHONY: all loader clean docs

all: loader

clean:
	@cargo clean

docs:
	@echo DOC
	@cargo doc

loader:
	@echo Build loader
	cargo build -Z build-std=core,alloc $(opt) --target $(target)-loader.json
	@llvm-objcopy --strip-debug -O elf32-i386 target/$(target)-loader/$(rdir)/rusty-loader
