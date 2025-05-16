BUILD ?= debug
TARGET ?= x86_64-unknown-none
CARGO_PROFILE ?= $(BUILD)

ifeq ($(BUILD), debug)
	CARGO_PROFILE = dev
endif

TARGET_ARCH = $(shell cut -d- -f1 <<< $(TARGET))
KERNEL = target/$(TARGET)/$(BUILD)/snek_kernel
ISO = out/$(TARGET)/$(BUILD)/snek_os.iso
OVMF = out/ovmf/OVMF.fd
LIMINE_BIN = out/limine/limine
LIMINE ?= out/limine
LIMINE_CFG = kernel/limine.cfg

.PHONY: all
all: $(ISO)

$(OVMF):
	mkdir -p out/ovmf
	curl -Lo out/ovmf/OVMF.fd https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF.fd
	curl -Lo out/ovmf/OVMF_CODE.fd https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF_CODE.fd
	curl -Lo out/ovmf/OVMF_VARS.fd https://retrage.github.io/edk2-nightly/bin/RELEASEX64_OVMF_VARS.fd

$(LIMINE_BIN):
	mkdir -p out
	cd out && git clone https://github.com/limine-bootloader/limine.git --branch=v5.x-branch-binary --depth=1
	cd out/limine && $(MAKE)

$(KERNEL): FORCE
	cargo build --profile $(CARGO_PROFILE) --package snek_kernel --target $(TARGET) --config kernel/config.toml

$(ISO): $(KERNEL) $(LIMINE_BIN) $(LIMINE_CFG)
	rm -rf out/iso_root
	mkdir -p out/iso_root
	mkdir -p out/$(TARGET)/$(BUILD)
	cp $(KERNEL) out/iso_root/kernel.elf
	cp $(LIMINE_CFG) out/iso_root/
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

.PHONY: clean format clippy run kernel iso

kernel: $(KERNEL)

iso: $(ISO)

clean:
	cargo clean
	rm -rf out

fmt:
	cargo fmt

lint:
	cargo clippy --package snek_kernel --target $(TARGET) --config kernel/config.toml

run: $(ISO) $(OVMF)
	"qemu-system-$(TARGET_ARCH)" \
		-machine q35 \
		-accel kvm \
		-cpu host \
		-debugcon /dev/stdout \
		-smp 4 \
		-m 8G \
		-rtc base=utc,clock=host \
		-drive file=nvm.img,if=none,id=nvm,format=raw \
		-device nvme,serial=deadbeef,drive=nvm \
		-netdev user,id=u1,ipv6=on,ipv4=on -device virtio-net,netdev=u1 \
		-device vhost-vsock-pci,guest-cid=3 \
		-drive format=raw,if=pflash,readonly=on,file=out/ovmf/OVMF_CODE.fd \
		-drive format=raw,if=pflash,file=out/ovmf/OVMF_VARS.fd \
		-drive file=$(ISO),format=raw
