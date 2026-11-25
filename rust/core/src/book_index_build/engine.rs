//! Formation and joins of the six typed streams.
use super::{*, records::{Record, Item, Reference, Joined, First, Spine},
    runs::{self, Io, Stream, Region, Pool, View, Output, sort, reduce, poll, io}};

pub(super) struct Engine<'a> {
    pub buffer: &'a mut [u8], pub io: Io, streams: [Stream; 6],
    pub min_read: usize, run_bytes: usize, members: usize, items: usize, references: usize, disk: bool,
}
/// Positions are read back from the card before their run CRC is checked, so
/// an out-of-range one is a misread: refuse it instead of indexing past the set.
pub(super) struct Positions(pub(super) [u64; MAX_SPINE as usize / 64]);
impl Positions {
    pub(super) fn mark(&mut self, position: u32) -> Result<(), Error> {
        *self.0.get_mut(position as usize / 64).ok_or_else(io)? |= 1 << (position % 64);
        Ok(())
    }
    pub(super) fn rank(&self, position: u32) -> Result<u32, Error> {
        let word = position as usize / 64;
        let partial = self.0.get(word).ok_or_else(io)? & ((1u64 << (position % 64)) - 1);
        Ok(self.0[..word].iter().map(|b| b.count_ones()).sum::<u32>() + partial.count_ones())
    }
    fn total(&self) -> u32 { self.0.iter().map(|b| b.count_ones()).sum() }
}
impl<'a> Engine<'a> {
    pub fn new(buffer: &'a mut [u8], min_read: usize, run_bytes: usize) -> Result<Self, Error> {
        if min_read < 24 || min_read > u16::MAX as usize || buffer.len() < 384 + 2 * min_read || buffer.len() < 3 * min_read + 24
            || buffer.len().div_ceil(min_read) > runs::CURSORS || run_bytes < 48 || run_bytes > buffer.len() {
            return Err(Error::Book(Status::MetadataTooLarge));
        }
        Ok(Self { buffer, io: Io::default(), streams: [Stream::default(); 6], min_read, run_bytes,
            members: 0, items: 0, references: 0, disk: false })
    }
    fn spill<W: WorkFile + ?Sized>(&mut self, work: &mut W) -> Result<(), Error> {
        for (kind, start, bytes) in [(0, 0, self.members), (1, self.members, self.items),
            (2, self.run_bytes - self.references, self.references)] {
            let b = &mut self.buffer[start..start + bytes];
            match kind {
                0 => { sort::<Member>(b, false); self.io.known::<Member, W>(work, &mut self.streams[0].list, b)?; }
                1 => { sort::<Item>(b, false); self.io.known::<Item, W>(work, &mut self.streams[1].list, b)?; }
                _ => { sort::<Reference>(b, false); self.io.known::<Reference, W>(work, &mut self.streams[2].list, b)?; }
            }
        }
        self.members = 0; self.items = 0; self.references = 0; self.disk = true;
        Ok(())
    }
    pub fn member<W: WorkFile + ?Sized>(&mut self, work: &mut W, record: Member) -> Result<(), Error> {
        if self.members + Member::BYTES > self.run_bytes { self.spill(work)?; }
        record.encode(&mut self.buffer[self.members..][..Member::BYTES]); self.members += Member::BYTES;
        Ok(())
    }
    pub fn item<W: WorkFile + ?Sized>(&mut self, work: &mut W, record: Item) -> Result<(), Error> {
        if self.members + self.items + self.references + Item::BYTES > self.run_bytes { self.spill(work)?; }
        record.encode(&mut self.buffer[self.members + self.items..][..Item::BYTES]); self.items += Item::BYTES;
        Ok(())
    }
    pub fn reference<W: WorkFile + ?Sized>(&mut self, work: &mut W, record: Reference) -> Result<(), Error> {
        if self.members + self.items + self.references + Reference::BYTES > self.run_bytes { self.spill(work)?; }
        self.references += Reference::BYTES;
        record.encode(&mut self.buffer[self.run_bytes - self.references..][..Reference::BYTES]);
        Ok(())
    }
    fn finish_input<W: WorkFile + ?Sized>(&mut self, work: &mut W) -> Result<(), Error> {
        if self.disk { self.spill(work)?; }
        else {
            let m = Region { start: 0, bytes: self.members };
            let i = Region { start: self.members, bytes: self.items };
            let r = Region { start: self.run_bytes - self.references, bytes: self.references };
            sort::<Member>(&mut self.buffer[m.start..][..m.bytes], true);
            sort::<Item>(&mut self.buffer[i.start..][..i.bytes], true);
            sort::<Reference>(&mut self.buffer[r.start..][..r.bytes], true);
            for (stream, region) in self.streams.iter_mut().zip([m, i, r]) { stream.memory = Some(region); }
        }
        Ok(())
    }
    pub fn join<W: WorkFile + ?Sized>(&mut self, work: &mut W, header: &mut Header,
        toc: Option<u64>, cover_id: Option<u64>, nav: Option<Choice>, mut ncx: Option<Choice>, mut cover: Option<Choice>,
        progress: &mut dyn FnMut(Progress) -> bool) -> Result<(), Error> {
        self.finish_input(work)?;
        let mut positions = Positions([0; MAX_SPINE as usize / 64]);
        self.join_items(work, toc, cover_id, &mut ncx, &mut cover, &mut positions, progress)?;
        header.spine = positions.total();
        if header.spine == 0 { return Err(Error::Book(Status::NotEpub)); }
        self.header_pass(work, header, nav, ncx, cover, &positions, progress)
    }
    fn join_items<W: WorkFile + ?Sized>(&mut self, work: &mut W, toc: Option<u64>, cover_id: Option<u64>,
        ncx: &mut Option<Choice>, cover: &mut Option<Choice>, positions: &mut Positions,
        progress: &mut dyn FnMut(Progress) -> bool) -> Result<(), Error> {
        let fan = (self.buffer.len() - Joined::BYTES) / self.min_read;
        let right = self.streams[2].slots().min(fan / 2).max(1);
        reduce::<Item, W>(&mut self.streams[1], fan - right, self.buffer, self.min_read, work, &mut self.io, progress)?;
        let left = self.streams[1].slots();
        reduce::<Reference, W>(&mut self.streams[2], fan - left, self.buffer, self.min_read, work, &mut self.io, progress)?;
        let mut pool = Pool::new(0, self.min_read);
        let mut items = pool.attach::<Item, W>(work, &mut self.io, self.streams[1])?;
        let mut refs = pool.attach::<Reference, W>(work, &mut self.io, self.streams[2])?;
        // A resident R region is reused in place for J1; spilled output gets the space after the cursors.
        let output = self.streams[2].memory
            .unwrap_or(Region { start: pool.bytes(), bytes: self.buffer.len() - pool.bytes() });
        let mut out = Output::<Joined>::new(output.start, output.bytes, !self.disk, false);
        let mut view = View { prefix: &[], tail: self.buffer };
        let mut item = pool.next::<Item, W>(&mut items, &mut view, work, &mut self.io)?;
        let mut reference = pool.next::<Reference, W>(&mut refs, &mut view, work, &mut self.io)?;
        let mut done = 0;
        while let Some(first) = item {
            let choice = Choice { sequence: first.sequence, hash: first.href };
            if Some(first.id) == toc { earlier(ncx, choice); }
            if Some(first.id) == cover_id { earlier(cover, choice); }
            while let Some(r) = reference.filter(|r| r.id <= first.id) {
                if r.id == first.id {
                    positions.mark(r.position)?;
                    out.push(Joined { href: first.href, position: r.position, text: first.flags & TEXT != 0 }, &mut view, work, &mut self.io)?;
                }
                reference = pool.next::<Reference, W>(&mut refs, &mut view, work, &mut self.io)?;
                done += 1; poll(progress, Phase::Join, done, 0)?;
            }
            while item.is_some_and(|item| item.id == first.id) {
                item = pool.next::<Item, W>(&mut items, &mut view, work, &mut self.io)?;
                done += 1; poll(progress, Phase::Join, done, 0)?;
            }
        }
        while reference.is_some() {
            reference = pool.next::<Reference, W>(&mut refs, &mut view, work, &mut self.io)?;
            done += 1; poll(progress, Phase::Join, done, 0)?;
        }
        self.streams[3] = out.finish(&mut view, work, &mut self.io)?;
        Ok(())
    }
    fn header_pass<W: WorkFile + ?Sized>(&mut self, work: &mut W, header: &mut Header, nav: Option<Choice>,
        ncx: Option<Choice>, cover: Option<Choice>, positions: &Positions, progress: &mut dyn FnMut(Progress) -> bool) -> Result<(), Error> {
        let f_bytes = self.min_read;
        let max_slots = (self.buffer.len() - f_bytes - 2 * Spine::BYTES) / self.min_read;
        reduce::<Member, W>(&mut self.streams[0], max_slots - 1, self.buffer, self.min_read, work, &mut self.io, progress)?;
        let m_slots = if self.disk { self.streams[0].slots() } else { 0 };
        let available = self.buffer.len() - m_slots * self.min_read - f_bytes;
        let reserve = (self.buffer.len() / 4).min(available - self.min_read);
        let j_slots = (available - reserve) / self.min_read;
        reduce::<Joined, W>(&mut self.streams[3], j_slots, self.buffer, self.min_read, work, &mut self.io, progress)?;
        let mut pool = Pool::new(0, self.min_read);
        let mut members = pool.attach::<Member, W>(work, &mut self.io, self.streams[0])?;
        let mut joins = pool.attach::<Joined, W>(work, &mut self.io, self.streams[3])?;
        let start = self.io.end;
        // Resident F and J2 reuse the I and J1 regions; spilled ones follow the cursors.
        let (f_region, j_region) = match (self.streams[1].memory, self.streams[3].memory) {
            (Some(items), Some(joined)) => (items, joined),
            _ => (Region { start: pool.bytes(), bytes: f_bytes },
                Region { start: pool.bytes() + f_bytes, bytes: self.buffer.len() - pool.bytes() - f_bytes }),
        };
        let mut first = Output::<First>::new(f_region.start, f_region.bytes, !self.disk, true);
        let mut spine = Output::<Spine>::new(j_region.start, j_region.bytes, !self.disk, false);
        let mut view = View { prefix: &[], tail: self.buffer };
        let mut joined = pool.next::<Joined, W>(&mut joins, &mut view, work, &mut self.io)?;
        let (mut rank, mut previous, mut valid) = (0, None, true);
        while let Some(member) = pool.next::<Member, W>(&mut members, &mut view, work, &mut self.io)? {
            if previous == Some(member.hash) { return Err(Error::Book(Status::Unsupported)); }
            previous = Some(member.hash);
            if nav.or(ncx).is_some_and(|c| c.hash == member.hash) {
                if nav.is_some() { header.nav = rank; } else { header.ncx = rank; }
            }
            if cover.is_some_and(|c| c.hash == member.hash) { header.cover = rank; }
            let mut first_position = NONE;
            while let Some(j) = joined.filter(|j| j.href <= member.hash) {
                let position = positions.rank(j.position)?;
                let found = j.href == member.hash;
                if found { first_position = first_position.min(position); }
                let weight = if j.text && found { member.uncompressed } else { 0 };
                if j.text && (!found || !matches!(member.method, 0 | 8) || member.uncompressed > 4 * 1024 * 1024) { valid = false; }
                header.spine_bytes += u64::from(weight);
                spine.push(Spine { position, member: if found { rank } else { NONE }, weight }, &mut view, work, &mut self.io)?;
                joined = pool.next::<Joined, W>(&mut joins, &mut view, work, &mut self.io)?;
                poll(progress, Phase::Join, u64::from(rank), u64::from(header.members))?;
            }
            if first_position != NONE { first.push(First { rank, position: first_position }, &mut view, work, &mut self.io)?; }
            rank += 1;
            poll(progress, Phase::Join, u64::from(rank), u64::from(header.members))?;
        }
        while let Some(j) = joined {
            if j.text { valid = false; }
            spine.push(Spine { position: positions.rank(j.position)?, member: NONE, weight: 0 }, &mut view, work, &mut self.io)?;
            joined = pool.next::<Joined, W>(&mut joins, &mut view, work, &mut self.io)?;
            poll(progress, Phase::Join, u64::from(rank), u64::from(header.members))?;
        }
        self.streams[4] = first.finish(&mut view, work, &mut self.io)?;
        self.streams[5] = spine.finish(&mut view, work, &mut self.io)?;
        if self.disk { self.streams[4].forward = Some((start, self.io.end)); }
        if !valid || header.spine_bytes > 64 * 1024 * 1024 { header.spine_bytes = 0; }
        Ok(())
    }
    pub fn publish<W: WorkFile + CacheStorage + ?Sized>(&mut self, store: &mut W, header: Header,
        progress: &mut dyn FnMut(Progress) -> bool, receipt_live: &mut dyn FnMut() -> bool) -> Result<(), Error> {
        use crate::book_index::{Writer, SpineItem};
        let fan = (self.buffer.len() - 384) / self.min_read;
        reduce::<Member, W>(&mut self.streams[0], fan - 1, self.buffer, self.min_read, store, &mut self.io, progress)?;
        reduce::<Spine, W>(&mut self.streams[5], fan, self.buffer, self.min_read, store, &mut self.io, progress)?;
        if self.disk && !store.sync_work() { return Err(io()); }
        let mut saved = [0; 384];
        saved.copy_from_slice(&self.buffer[..384]);
        let (write_buffer, tail) = self.buffer.split_at_mut(384);
        let mut view = View { prefix: &saved, tail };
        let mut pool = Pool::new(384, self.min_read);
        let mut members = pool.attach::<Member, W>(store, &mut self.io, self.streams[0])?;
        let mut first = pool.attach::<First, W>(store, &mut self.io, self.streams[4])?;
        let mut f = pool.next::<First, W>(&mut first, &mut view, store, &mut self.io)?;
        let mut writer = Writer::begin(store, header, write_buffer).map_err(Error::Write)?;
        let mut rank = 0;
        while let Some(mut member) = pool.next::<Member, W>(&mut members, &mut view, store, &mut self.io)? {
            if f.is_some_and(|f| f.rank < rank) { return Err(io()); }
            if let Some(next) = f.filter(|f| f.rank == rank) {
                member.first_spine = next.position;
                f = pool.next::<First, W>(&mut first, &mut view, store, &mut self.io)?;
            }
            writer = writer.member(store, member).map_err(Error::Write)?;
            rank += 1; poll(progress, Phase::Publish, u64::from(rank), u64::from(header.members + header.spine))?;
        }
        if f.is_some() || rank != header.members { return Err(io()); }
        drop(pool);
        let mut pool = Pool::new(384, self.min_read);
        let mut spine = pool.attach::<Spine, W>(store, &mut self.io, self.streams[5])?;
        let mut before = 0;
        let mut count = 0;
        while let Some(s) = pool.next::<Spine, W>(&mut spine, &mut view, store, &mut self.io)? {
            if s.position != count { return Err(io()); }
            writer = writer.spine(store, SpineItem { member: s.member, before: if header.spine_bytes == 0 { 0 } else { before } }).map_err(Error::Write)?;
            before += u64::from(s.weight); count += 1;
            poll(progress, Phase::Publish, u64::from(rank + count), u64::from(header.members + header.spine))?;
        }
        writer.finish(store, receipt_live).map_err(Error::Write)
    }
}
pub(super) fn fixed_state_bytes() -> usize {
    // Package is gone before positions and merge cursors are constructed.
    core::mem::size_of::<Engine<'_>>() + core::mem::size_of::<Positions>() + core::mem::size_of::<Pool>()
        + core::mem::size_of::<Header>() + 2 * core::mem::size_of::<runs::Group>()
        + core::mem::size_of::<Output<Spine>>() + core::mem::size_of::<Output<First>>()
        + core::mem::size_of::<(Option<Member>, Option<Joined>, Option<First>, Option<Choice>, Option<Choice>, u64, u32, u32, bool)>()
}
#[cfg(test)]
pub(super) fn state_sizes() -> [usize; 7] {
    [core::mem::size_of::<Engine<'_>>(), core::mem::size_of::<Positions>(), core::mem::size_of::<Pool>(),
     core::mem::size_of::<Header>(), core::mem::size_of::<runs::Group>(), core::mem::size_of::<Output<Spine>>(),
     core::mem::size_of::<(Option<Member>, Option<Joined>, Option<First>, Option<Choice>, Option<Choice>, u64, u32, u32, bool)>()]
}
