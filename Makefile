.PHONY: all build test test-cli test-e2e clean install uninstall site

all: build

build:
	cargo build --release

test:
	cargo test
	./tests/cli.sh

test-cli:
	./tests/cli.sh

test-e2e:
	./tests/e2e/sdme.sh
	./tests/e2e/sdme-jump.sh

clean:
	cargo clean
	cd site && zola clean 2>/dev/null || rm -rf public

install:
	cargo install --path . --locked

uninstall:
	cargo uninstall ush

site:
	cd site && zola build
