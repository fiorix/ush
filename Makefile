.PHONY: all build test clean install uninstall site

all: build

build:
	cargo build --release

test:
	cargo test

clean:
	cargo clean
	cd site && zola clean 2>/dev/null || rm -rf public

install:
	cargo install --path . --locked

uninstall:
	cargo uninstall ush

site:
	cd site && zola build
