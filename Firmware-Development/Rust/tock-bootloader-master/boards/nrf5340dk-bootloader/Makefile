TOCK_ARCH=cortex-m4
TARGET=thumbv8m.main-none-eabi
PLATFORM=nrf5340dk-bootloader

include ../Common.mk

JLINK=JLinkExe
JLINK_DEVICE=nRF5340_xxAA_APP
JLINK_IF=SWD
JLINK_SPEED=4000
JLINK_CMD_FILE=jlink_cmd.jlink

# Create a JLink command file
define make_jlink_cmd
	echo "device $(JLINK_DEVICE)" > $(JLINK_CMD_FILE); \
	echo "speed $(JLINK_SPEED)" >> $(JLINK_CMD_FILE); \
	echo "if $(JLINK_IF)" >> $(JLINK_CMD_FILE); \
	echo "connect" >> $(JLINK_CMD_FILE); \
	echo "r" >> $(JLINK_CMD_FILE); \
	echo "loadbin $<, 0x000" >> $(JLINK_CMD_FILE); \
	echo "r" >> $(JLINK_CMD_FILE); \
	echo "g" >> $(JLINK_CMD_FILE); \
	echo "exit" >> $(JLINK_CMD_FILE)
endef

.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).bin
	$(make_jlink_cmd); \
	$(JLINK) -CommandFile $(JLINK_CMD_FILE)

.PHONY: flash-debug
flash-debug: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/debug/$(PLATFORM).bin
	$(make_jlink_cmd); \
	$(JLINK) -CommandFile $(JLINK_CMD_FILE)
