arch ?= x86_64
target ?= $(arch)-unknown-hermit
release ?= 0

opt :=
rdir := debug

ifeq ($(release), 1)
opt := --release
rdir := release
endif

CONVERT :=
RN :=
ifdef COMSPEC
RM := del
else
RM := rm -rf
endif
SYSROOT := $(shell rustc --print sysroot)
OBJCOPY := $(shell find $(SYSROOT) -name llvm-objcopy)
ifeq ($(arch), x86_64)
CONVERT := $(OBJCOPY) --strip-debug -O elf32-i386 target/$(target)-loader/$(rdir)/rusty-loader
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
	cargo build $(opt) --target $(target)-loader.json
	$(CONVERT)
