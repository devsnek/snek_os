BUILD ?= debug
TARGET ?= x86_64-unknown-none
CARGO_PROFILE ?= $(BUILD)

ifeq ($(BUILD), debug)
	CARGO_PROFILE = dev
endif

KERNEL = target/$(TARGET)/$(BUILD)/snek_kernel
ISO = out/$(TARGET)/$(BUILD)/snek_os.iso
OVMF = out/ovmf/OVMF.fd
LIMINE_BIN = out/limine/limine
LIMINE ?= out/limine

.PHONY: all
all: $(ISO)

$(OVMF):
	mkdir -p out/ovmf
	cd out/ovmf && curl -Lo OVMF.fd https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF.fd

$(LIMINE_BIN):
	mkdir -p out
	cd out && git clone https://github.com/limine-bootloader/limine.git --branch=v5.x-branch-binary --depth=1
	cd out/limine && $(MAKE)

$(KERNEL): FORCE
	cargo build --profile $(CARGO_PROFILE) --package snek_kernel --target $(TARGET)

$(ISO): $(KERNEL) $(OVMF) $(LIMINE_BIN)
	rm -rf out/iso_root
	mkdir -p out/iso_root
	mkdir -p out/$(TARGET)/$(BUILD)
	cp $(KERNEL) out/iso_root/kernel.elf
	cp kernel/limine.cfg out/iso_root/
	cp -v $(LIMINE)/limine-bios.sys $(LIMINE)/limine-bios-cd.bin $(LIMINE)/limine-uefi-cd.bin out/iso_root/
	mkdir -p out/iso_root/EFI/BOOT
	cp -v $(LIMINE)/BOOTX64.EFI out/iso_root/EFI/BOOT/
	xorriso -as mkisofs -b limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		out/iso_root -o $(ISO)
	$(LIMINE_BIN) bios-install $(ISO)

FORCE: ;

.PHONY: clean format clippy run
clean:
	cargo clean
	rm -rf out

fmt:
	cargo fmt

clippy:
	cargo clippy --package snek_kernel --target $(TARGET)

run: $(ISO)
	qemu-system-x86_64 \
		-enable-kvm \
		-cpu host \
		-M q35 \
		-debugcon /dev/stdout \
		-smp 4 \
		-m 4G \
		-rtc base=utc,clock=host \
		-device qemu-xhci \
		-netdev user,id=u1 -device e1000,netdev=u1 \
		-bios $(OVMF) \
		-drive file=$(ISO),format=raw
