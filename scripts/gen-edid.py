#!/usr/bin/env python3
"""
Generate EDID binary for Samsung Galaxy Tab S9 Ultra (2960x1848@60).
Uses correct DTD encoding matching Linux kernel's detailed_pixel_timing struct.

DTD byte layout (offset from DTD start):
  0-1: Pixel clock (little-endian, 10kHz units)
  2:   H_Active[7:0]
  3:   H_Blank[7:0]
  4:   bits 7-4: H_Active[11:8], bits 3-0: H_Blank[11:8]
  5:   V_Active[7:0]
  6:   V_Blank[7:0]
  7:   bits 7-4: V_Active[11:8], bits 3-0: V_Blank[11:8]
  8:   H_Sync_Offset[7:0]
  9:   H_Sync_Width[7:0]
  10:  bits 7-4: V_Sync_Offset[3:0], bits 3-0: V_Sync_Width[3:0]
  11:  bits 7-6: H_Sync_Offset[10:8], bits 5-4: H_Sync_Width[10:8],
        bits 3-2: V_Sync_Offset[5:4], bits 1-0: V_Sync_Width[5:4]
  12:  H_Image_Size[7:0]
  13:  V_Image_Size[7:0]
  14:  bits 7-4: H_Image_Size[11:8], bits 3-0: V_Image_Size[11:8]
  15:  H_Border
  16:  V_Border
  17:  misc flags
"""
import struct
import sys


def encode_manufacturer_id(s):
    """Encode 3-letter manufacturer ID into EDID bytes 8-9.
    Each character is encoded as 5 bits (A=1, B=2, ..., Z=26),
    packed big-endian into 2 bytes with the MSB of byte 8 = 0.
    """
    assert len(s) == 3 and s.isalpha() and s.isupper(), f"Invalid manufacturer ID: {s}"
    c1 = ord(s[0]) - ord('A') + 1  # 1-26
    c2 = ord(s[1]) - ord('A') + 1
    c3 = ord(s[2]) - ord('A') + 1
    # Byte 8: 0bbb bbcc  (b=c1[4:0], c=c2[4:3])
    # Byte 9: 0bcc ccdd  (c=c2[2:0], d=c3[4:0])
    byte8 = ((c1 & 0x1F) << 2) | ((c2 >> 3) & 0x03)
    byte9 = ((c2 & 0x07) << 5) | (c3 & 0x1F)
    return byte8, byte9


# Keep in sync with common/src/lib.rs; cross-language Cargo regressions verify it.
MIN_FPS, MAX_FPS = 10, 90


def validate_dimensions(width, height, refresh, width_mm, height_mm):
    if not (1 <= width <= 4095 and 1 <= height <= 4095):
        raise ValueError("EDID active dimensions must fit 12 bits and be positive")
    if not MIN_FPS <= refresh <= MAX_FPS:
        raise ValueError(f"EDID refresh must be within {MIN_FPS}..{MAX_FPS} Hz")
    if not (1 <= width_mm <= 4095 and 1 <= height_mm <= 4095):
        raise ValueError("EDID physical dimensions must fit 12 bits and be positive")


def make_edid(width, height, refresh=60, name="Blent", width_mm=310, height_mm=194):
    validate_dimensions(width, height, refresh, width_mm, height_mm)
    edid = bytearray(128)

    # Header
    edid[0:8] = b'\x00\xFF\xFF\xFF\xFF\xFF\xFF\x00'

    # Manufacturer ID (BLN = Blent)
    edid[8], edid[9] = encode_manufacturer_id("BLN")

    # Product code
    edid[10] = 0x01
    edid[11] = 0x00

    # Serial number
    edid[12:16] = struct.pack('<I', 1)

    edid[16] = 0   # week
    edid[17] = 34  # year (2024-1990)

    edid[18] = 1   # version
    edid[19] = 4   # revision

    # Digital input (8-bit color depth, DisplayPort interface code 5)
    edid[20] = 0xA5  # Digital, 8 bpc, DisplayPort

    # Max image size (cm)
    edid[21] = max(1, min(255, width_mm // 10))
    edid[22] = max(1, min(255, height_mm // 10))

    # Gamma
    edid[23] = 0x78  # gamma = 2.2

    # DPMS / features
    edid[24] = 0xEE  # RGB, DPMS active-off/suspend/standby

    # Chromaticity (sRGB), all eight packed 10-bit coordinates.
    edid[25:35] = bytes([0xEE, 0x91, 0xA3, 0x54, 0x4C, 0x99, 0x26, 0x0F, 0x50, 0x54])

    # Established timings (none)
    edid[35] = 0x00
    edid[36] = 0x00
    edid[37] = 0x00

    # Standard timings (none)
    for i in range(8):
        edid[38 + i * 2] = 0x01
        edid[39 + i * 2] = 0x01

    # Custom fixed-porch timings; these do not implement the CVT-RB formulas.
    h_active = width
    v_active = height

    h_front = 48
    h_sync = 32
    h_back = 80
    h_blank = h_front + h_sync + h_back

    v_front = 3
    v_sync = 10
    v_back = 25
    v_blank = v_front + v_sync + v_back

    h_total = h_active + h_blank
    v_total = v_active + v_blank

    # Pixel clock in 10kHz units
    pixel_clock_10khz = (h_total * v_total * refresh + 5000) // 10000

    minimum = max(MIN_FPS, (9_995_000 + h_total * v_total - 1) // (h_total * v_total))
    if pixel_clock_10khz < 1000:
        raise ValueError(f"EDID pixel clock is below the supported 10 MHz minimum; use at least {minimum} Hz (maximum {MAX_FPS} Hz), or increase the resolution")
    if pixel_clock_10khz > 65535:
        raise ValueError("EDID pixel clock must fit 16 bits (maximum 655.35 MHz)")
    h_image = width_mm
    v_image = height_mm

    # === DTD 1 (bytes 54-71) ===
    idx = 54
    # Bytes 0-1: Pixel clock (LE, 10kHz)
    edid[idx:idx+2] = struct.pack('<H', pixel_clock_10khz)

    # Byte 2: H_Active[7:0]
    edid[idx+2] = h_active & 0xFF

    # Byte 3: H_Blank[7:0]
    edid[idx+3] = h_blank & 0xFF

    # Byte 4: bits 7-4 = H_Active[11:8], bits 3-0 = H_Blank[11:8]
    edid[idx+4] = ((h_active >> 8) & 0x0F) << 4 | ((h_blank >> 8) & 0x0F)

    # Byte 5: V_Active[7:0]
    edid[idx+5] = v_active & 0xFF

    # Byte 6: V_Blank[7:0]
    edid[idx+6] = v_blank & 0xFF

    # Byte 7: bits 7-4 = V_Active[11:8], bits 3-0 = V_Blank[11:8]
    edid[idx+7] = ((v_active >> 8) & 0x0F) << 4 | ((v_blank >> 8) & 0x0F)

    # Byte 8: H_Sync_Offset[7:0]
    edid[idx+8] = h_front & 0xFF

    # Byte 9: H_Sync_Width[7:0]
    edid[idx+9] = h_sync & 0xFF

    # Byte 10: bits 7-4 = V_Sync_Offset[3:0], bits 3-0 = V_Sync_Width[3:0]
    edid[idx+10] = ((v_front & 0x0F) << 4) | (v_sync & 0x0F)

    # Byte 11:
    #   bits 7-6 = H_Sync_Offset[10:8]
    #   bits 5-4 = H_Sync_Width[10:8]
    #   bits 3-2 = V_Sync_Offset[5:4]
    #   bits 1-0 = V_Sync_Width[5:4]
    edid[idx+11] = (((h_front >> 8) & 0x03) << 6) | \
                   (((h_sync >> 8) & 0x03) << 4) | \
                   (((v_front >> 4) & 0x03) << 2) | \
                   ((v_sync >> 4) & 0x03)

    # Byte 12: H_Image_Size[7:0]
    edid[idx+12] = h_image & 0xFF

    # Byte 13: V_Image_Size[7:0]
    edid[idx+13] = v_image & 0xFF

    # Byte 14: bits 7-4 = H_Image_Size[11:8], bits 3-0 = V_Image_Size[11:8]
    edid[idx+14] = (((h_image >> 8) & 0x0F) << 4) | ((v_image >> 8) & 0x0F)

    # Byte 15: H_Border = 0
    # Byte 16: V_Border = 0
    # Byte 17: misc = 0x1E (non-interlaced + digital separate sync + positive h/v)
    edid[idx+15] = 0
    edid[idx+16] = 0
    edid[idx+17] = 0x1E

    # === Unused descriptor (bytes 72-89): EDID dummy tag, zero payload ===
    edid[75] = 0x10

    # === Monitor name descriptor (bytes 90-107) ===
    idx = 90
    edid[idx] = 0x00
    edid[idx+1] = 0x00
    edid[idx+2] = 0x00
    edid[idx+3] = 0xFC  # Monitor name tag
    edid[idx+4] = 0x00  # Reserved
    name_str = name[:12]  # max 12 chars + newline
    name_bytes = name_str.encode('ascii', errors='replace')
    name_padded = name_bytes + b'\x0A'  # LF terminates the string
    name_padded = name_padded.ljust(13, b'\x20')[:13]
    edid[idx+5:idx+18] = name_padded

    # === Monitor range limits descriptor (bytes 108-125) ===
    idx = 108
    edid[idx] = 0x00
    edid[idx+1] = 0x00
    edid[idx+2] = 0x00
    edid[idx+3] = 0xFD  # Range limits tag
    edid[idx+5] = minimum
    edid[idx+6] = MAX_FPS
    clock_hz = pixel_clock_10khz * 10000
    min_h = min(v_total * minimum // 1000, clock_hz // (h_total * 1000))
    max_h = max((v_total * MAX_FPS + 999) // 1000,
                (clock_hz + h_total * 1000 - 1) // (h_total * 1000))
    edid[idx+4] = ((min_h > 255) << 2) | ((max_h > 255) << 3)
    edid[idx+7] = min_h - (255 if min_h > 255 else 0)
    edid[idx+8] = max_h - (255 if max_h > 255 else 0)
    # Max pixel clock in MHz / 10, rounded up
    max_pclk_mhz = (pixel_clock_10khz * 10000 + 999999) // 1000000
    edid[idx+9] = (max_pclk_mhz + 9) // 10  # Round up to nearest 10MHz
    edid[idx+10] = 0x01  # EDID 1.4: range limits only, no timing formula
    edid[idx+11:idx+18] = b'\x0A\x20\x20\x20\x20\x20\x20'

    # Extension flag
    edid[126] = 0

    # Checksum
    edid[127] = (256 - sum(edid[:127]) % 256) % 256

    return bytes(edid)


def main():
    width = int(sys.argv[1]) if len(sys.argv) > 1 else 2960
    height = int(sys.argv[2]) if len(sys.argv) > 2 else 1848
    refresh = int(sys.argv[3]) if len(sys.argv) > 3 else 60
    output = sys.argv[4] if len(sys.argv) > 4 else "s9ultra.bin"

    width_mm = int(sys.argv[5]) if len(sys.argv) > 5 else 310
    height_mm = int(sys.argv[6]) if len(sys.argv) > 6 else 194
    edid = make_edid(width, height, refresh, width_mm=width_mm, height_mm=height_mm)
    with open(output, 'wb') as f:
        f.write(edid)

    checksum = sum(edid) % 256
    print(f"EDID generated: {output}")
    print(f"  Resolution: {width}x{height} @ {refresh}Hz")
    print(f"  Size: {len(edid)} bytes")
    print(f"  Pixel clock: {struct.unpack('<H', edid[54:56])[0] / 100:.2f} MHz")
    print(f"  Checksum: {checksum} {'OK' if checksum == 0 else 'BAD'}")

    # Verify DTD
    h_act = edid[56] | ((edid[58] >> 4) << 8)
    v_act = edid[59] | ((edid[61] >> 4) << 8)
    print(f"  DTD active: {h_act}x{v_act}")


if __name__ == '__main__':
    main()
