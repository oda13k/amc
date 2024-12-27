
PREFIX ?= ${HOME}/.local

.PHONY: all
all:
	cargo build

.PHONY: run
run:
	cargo run

.PHONY: install
install:
	cargo install --path . --root ${PREFIX}

.PHONY: uninstall
uninstall:
	cargo uninstall --root ${PREFIX}

.PHONY: clean
clean:
	cargo clean

