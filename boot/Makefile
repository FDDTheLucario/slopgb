# Build the slopgb CGB boot ROM (2304 bytes) with RGBDS.
TARGET = slopgb_cgb_boot.bin

$(TARGET): slopgb_cgb_boot.asm hardware.inc
	rgbasm -o boot.o slopgb_cgb_boot.asm
	rgblink -p 0x00 -o $(TARGET) boot.o
	@truncate -s 2304 $(TARGET)   # keep only the CGB-class region ($0000-$08FF)
	@sz=$$(stat -c%s $(TARGET)); \
	 if [ "$$sz" != "2304" ]; then echo "ERROR: $(TARGET) is $$sz bytes, expected 2304"; exit 1; fi; \
	 echo "built $(TARGET): $$sz bytes"

clean:
	rm -f boot.o $(TARGET)

.PHONY: clean
