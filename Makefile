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

.PHONY: all tests clippy clean lib docs

default: lib
	make arch=$(arch) release=$(release) -C tests

all: lib
	make arch=$(arch) release=$(release) -C tests

clean:
	$(RM) target/x86_64-unknown-hermit-kernel
	make -C tests clean

docs:
	@echo DOC
	@cargo doc --no-deps

clippy:
	@echo Run clippy...
	@RUST_TARGET_PATH=$(CURDIR) cargo clippy --target $(target)

lib:
	@echo Build libhermit
	@RUST_TARGET_PATH=$(CURDIR) cargo xbuild $(opt) --target $(target)-kernel
	@$(arch)-hermit-elfedit --output-osabi Standalone target/$(target)-kernel/$(rdir)/libhermit.a
