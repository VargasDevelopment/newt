# Borrowed raw-table contract

Inputs are the bytes of a single GPOS or GSUB table, never a whole font.
The table and sequence are borrowed for the call only; outputs own no storage.
No allocation, unsafe code, dependencies, cached state or font identity logic.
Public glyph inputs retain all 32 bits; they are never narrowed before matching
16-bit table glyphs. Pair glyph 0 returns 0, as in the original. A GSUB sequence
must have exactly 2 or 3 entries. Result 0 means no effective substitution;
kerning is signed FUnits, not scaled pixels.

ScriptRecords and pair/coverage format-1 entries must be sorted for meaningful
lookup. Binary search is bounded even for unsorted input. Prefer a usable
latn DefaultLangSys; fall back to DFLT only if Latin is unusable, never merely
because Latin yields no result. Named language systems are ignored. Process
required feature first (unless 0xffff), then referenced features in order.
Activate only kern for GPOS, liga/rlig for GSUB. Features in the pool alone do
not activate. No arbitrary cap on script, feature, lookup, pair-set indices.
Coverage formats 1/2 and class definitions self-cap at 8192. Outer search,
feature, lookup, subtable and ligature work share the original 4096 budget.

PairPos formats 1/2 consume two bytes per set ValueFormat bit, including all
reserved/high/device fields. Return only first-record signed XAdvance; ignore
placement and second-record values while respecting their widths. A matched
zero stops the entire GPOS query. A zero GSUB ligature stops its feature's
lookup walk but the outer feature walk may continue. Dense class-0 matrix
matches are real matches, including zero. Wrappers 9/7 must have format 1,
correct wrapped type and a valid nonzero 32-bit relative offset. No recursion.

Zero offsets are NULL; child views extend to the parent end as in the source,
not to an inferred sibling boundary. Truncated reads return zero and malformed
structures usually yield no result. Malformed class definitions can resolve
class 0 by design; this is original behavior, not a structural validity API.

Parity domain: valid Rust slices of at most u32::MAX bytes, valid supplied
sequence storage, and source offset/count expressions representable on RV32
size_t. Null original pointers correspond to absent/empty borrowed slices;
Rust exposes no raw-pointer API. All bounded index math is small except the
class matrix product. That product uses checked u32 multiplication, checked
usize record offsets and complete matrix bounds before access. A product that
would wrap RV32 size_t is rejected on both Rust hosts and targets. Such malformed
inputs are outside portable C++ host/RV32 parity: unsigned size_t wraps on RV32,
while 64-bit hosts can reject a larger nonwrapped bound. We do not claim that
host C++ parity proves original RV32 overflow behavior. No narrowing of glyph
input or wrapping arithmetic is used to manufacture parity. Valid byte-domain
reads and all representable source arithmetic retain original ordering/results.
