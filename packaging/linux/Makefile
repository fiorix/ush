# Build standalone release tarballs for ush.
#
# Usage:
#   make -C packaging/linux tarball LINUX_TARGET=x86_64-unknown-linux-musl

USH_REPO ?= ../..
CARGO ?= cargo
TAR ?= tar
LINUX_TARGET ?= x86_64-unknown-linux-musl

USH_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' "$(USH_REPO)/Cargo.toml" | head -1)
TARBALL_NAME := ush-$(LINUX_TARGET).tar.gz
TARBALL_STAGE := $(USH_REPO)/target/package/ush-$(LINUX_TARGET)
TARBALL_PATH := $(USH_REPO)/target/release/$(TARBALL_NAME)

# Use cargo-zigbuild for musl so the binary is fully static.
ifneq ($(filter %-unknown-linux-musl,$(LINUX_TARGET)),)
TARBALL_BUILD := $(CARGO) zigbuild
else
TARBALL_BUILD := $(CARGO) build
endif

.PHONY: tarball
tarball:
	@case "$(LINUX_TARGET)" in \
		x86_64-unknown-linux-musl|aarch64-unknown-linux-musl|aarch64-apple-darwin) ;; \
		*) echo "error: unsupported LINUX_TARGET: $(LINUX_TARGET)"; exit 1;; \
	esac
	cd "$(USH_REPO)" && $(TARBALL_BUILD) --release --target "$(LINUX_TARGET)"
	rm -rf "$(TARBALL_STAGE)"
	mkdir -p "$(TARBALL_STAGE)" "$(USH_REPO)/target/release"
	cp "$(USH_REPO)/target/$(LINUX_TARGET)/release/ush" "$(TARBALL_STAGE)/ush"
	cp "$(USH_REPO)/LICENSE" "$(TARBALL_STAGE)/LICENSE"
	cp "$(USH_REPO)/README.md" "$(TARBALL_STAGE)/README.md"
	$(TAR) -czf "$(TARBALL_PATH)" -C "$(TARBALL_STAGE)" .
	@echo "built $(TARBALL_PATH)"

.PHONY: all-tarballs
all-tarballs:
	$(MAKE) tarball LINUX_TARGET=x86_64-unknown-linux-musl
	$(MAKE) tarball LINUX_TARGET=aarch64-unknown-linux-musl
	$(MAKE) tarball LINUX_TARGET=aarch64-apple-darwin
