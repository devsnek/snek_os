BUILD ?= debug
PROFILE ?= $(BUILD)

ifeq ($(BUILD), debug)
	PROFILE = dev
endif

KERNEL = target/x86_64-unknown-none/$(BUILD)/snek_kernel
ISO = out/$(BUILD)/snek_os.iso
LIMINE = out/limine/limine
OVMF = out/ovmf/OVMF.fd

.PHONY: all
all: $(ISO)

$(OVMF):
	mkdir -p out/ovmf
	cd out/ovmf && curl -Lo OVMF.fd https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF.fd

$(LIMINE):
	mkdir -p out
	cd out && git clone https://github.com/limine-bootloader/limine.git --branch=v5.x-branch-binary --depth=1
	cd out/limine && $(MAKE)

$(KERNEL): FORCE
	cargo build --profile $(PROFILE) --package snek_kernel --target x86_64-unknown-none

$(ISO): $(KERNEL) $(OVMF) $(LIMINE)
	rm -rf out/iso_root
	mkdir -p out/iso_root
	mkdir -p out/$(BUILD)
	cp $(KERNEL) out/iso_root/kernel.elf
	cp kernel/limine.cfg out/iso_root/
	cp -v out/limine/limine-bios.sys out/limine/limine-bios-cd.bin out/limine/limine-uefi-cd.bin out/iso_root/
	mkdir -p out/iso_root/EFI/BOOT
	cp -v out/limine/BOOTX64.EFI out/iso_root/EFI/BOOT/
	cp -v out/limine/BOOTIA32.EFI out/iso_root/EFI/BOOT/
	xorriso -as mkisofs -b limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		out/iso_root -o $(ISO)
	./out/limine/limine bios-install $(ISO)

FORCE: ;

.PHONY: clean format clippy run
clean:
	cargo clean
	rm -rf out

fmt:
	cargo fmt

clippy:
	cargo clippy --package snek_kernel --target x86_64-unknown-none

run: $(ISO)
	qemu-system-x86_64 \
		-enable-kvm \
		-M q35 \
		-debugcon /dev/stdout \
		--no-shutdown --no-reboot \
		-smp 4 \
		-m 2G \
		-device qemu-xhci \
		-bios $(OVMF) \
		-drive file=$(ISO),format=raw
