arch ?= x86_64
target ?= $(arch)-unknown-hermit-kernel
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

.PHONY: all clean debug cargo docs demo

all: cargo

clean:
	$(RM) target

docs:
	@echo DOC
	@mv .cargo cargo
	@cargo doc
	@mv cargo .cargo

cargo:
	@echo CARGO
	@cargo xbuild $(opt) --target $(target).json --release
