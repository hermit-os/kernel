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

.PHONY: all clippy clean lib docs

all: lib
	make arch=$(arch) release=$(release) -C examples

clean:
	$(RM) target
	make -C examples clean

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
