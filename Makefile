BUILD ?= debug
PROFILE ?= $(BUILD)

ifeq ($(BUILD), debug)
	PROFILE = dev
endif

KERNEL = target/x86_64-unknown-none/$(BUILD)/snek_kernel
OS = target/$(BUILD)/snek_os

OS: KERNEL FORCE
	env CARGO_BIN_FILE_SNEK_KERNEL_snek_kernel=$(KERNEL) \
	cargo build --profile $(PROFILE) --package snek_os

KERNEL: FORCE
	cargo build --profile $(PROFILE) --package snek_kernel --target x86_64-unknown-none

FORCE: ;

.PHONY: clean format
clean:
	cargo clean

format:
	cargo fmt

clippy:
	cargo clippy --package snek_kernel --target x86_64-unknown-none
