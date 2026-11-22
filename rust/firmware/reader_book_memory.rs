// Newt MIT; the sole caller-owned book memory loan.
/// Boot-wide reader memory, lent to one book: the shared bank, selections,
/// page plan and compact receipt scratch.
pub struct BookMemory<'m, 'p> {
    pub bank: &'m mut [u8; BANK_BYTES],
    pub selections: &'m mut [Selection; 2],
    pub plan: &'m mut Plan<'p>,
    pub receipt: &'m mut [u8; 256],
    pub producer_ledger: &'m mut [u8; 256],
}
