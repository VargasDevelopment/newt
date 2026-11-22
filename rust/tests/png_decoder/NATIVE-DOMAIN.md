This is a faithful pinned-source port, not an automatic PNG conformance fix.
The pinned PNGLE_CHUNK_bKGD numeric constant is0x424b4744 (uppercase BKGD).
Standard lowercase bKGD is ignored; an earlier Rust mismatch was corrected.
The uppercase RGB/RGBA branch writes six samples into native v[4]. Rust stalls
on incomplete input first, then rejects immediately before that undefined path.
Pre-IHDR background follows the same incomplete-input ordering before safe
rejection. Empty recognized chunks can advance directly to their CRC.

The unmodified native read_uint32 promotes p[0] to signed int before <<24.
UBSan rejects ordinary high-bit CRC bytes, including every standard IEND CRC.
The comparison oracle uses a separately copied, hash-recorded explicit uint32_t
promotion. Frozen originals and original UBSan failure remain untouched; these
are comparisons against that narrow normalized oracle, not pristine native proof.

Enabled nonzero gAMA followed by zero gAMA leaves the native freed table pointer
dangling; Rust clears gamma_len. That is an explicit native undefined domain,
not an equality witness. Target gamma math remains an external borrowed seam.
CallbackError is a distinct type, so a callback cannot return decoder-owned
resumable Capacity. Callback failures are sticky through subsequent buffer loans.
