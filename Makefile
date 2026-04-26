BINARY := jig

.PHONY: build install clean test lint fmt check

build:
	cargo build --release
	cp target/release/$(BINARY) $(BINARY)

install:
	cargo install --path .

clean:
	cargo clean
	rm -f $(BINARY)

test: build
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

check: build
	./$(BINARY) --help > /dev/null
	./$(BINARY) check examples/agent-shape.example.toml > /dev/null
