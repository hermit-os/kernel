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

.PHONY: default clean

default:
	@RUSTFLAGS="-L ../target/$(target)-kernel/$(rdir)" cargo build $(opt) --target $(target)

clean:
	@$(RM) target
