RM = rm -rf
#TARGET =
TARGET = --target x86_64-unknown-hermit
#TARGET = --target x86_64-unknown-linux-gnu

default:
	cargo build $(TARGET)

test:
	cargo test --no-run $(TARGET)

bench:
	cargo bench --no-run $(TARGET)

clean:
	$(RM) target
