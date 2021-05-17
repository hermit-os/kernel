arch ?= x86_64
target ?= $(arch)-unknown-hermit
release ?= 0
app ?= "$(PWD)/data/hello_world"

opt :=
rdir := debug

ifeq ($(release), 1)
opt := --release
rdir := release
endif

# Todo - make this feature toggleable
ifeq ($(arch), aarch64)
opt += --features "aarch64-qemu-stdout"
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
	HERMIT_APP=$(app) cargo build $(opt) -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem --target $(target)-loader.json
	$(CONVERT)
