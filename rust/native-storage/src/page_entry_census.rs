//! Chapter cache page-entry census over a directory walker. MIT.
//! Directory/Native keep the existing host UTF-8 pathname contract. The SDK's
//! FatFs metadata walk is a later native boundary; no SDK cost is claimed here.
#![forbid(unsafe_code)]
use crate::{Authority, Error, Kind, MissingProbe, Native};
use crate::page_entries::{PageEntries, PageEntry as Entry};
use newt_core::{cache_names::namespace::{self as names, Form, Kind as NameKind, PageCacheNeed, PageName, PageNameSet}, reader_scheduler::YieldCadence};

/// One quota folder of block-envelope cache files, evicted oldest-written
/// first (FAT does not refresh times on reads). Other known cache names found
/// there rank as legacy and go first.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub folder: &'static str,
    pub kind: NameKind,
    pub quota: u64,
    /// Hard file cap; `entry_grant` leaves a reserve below it.
    pub entry_cap: u32,
    pub entry_grant: u8,
    /// Free credit units a writer needs for one largest file.
    pub required_units: u8,
    /// Pages keep a `.bak` and its canonical file. An index `.bak` is just an
    /// older copy left by a rebuild, evicted by age like any index; only the
    /// open book's pair is kept.
    pub keep_backups: bool,
}
pub const PAGES: Policy = Policy {
    folder: "/Pages", kind: NameKind::Page, quota: names::PAGE_QUOTA,
    entry_cap: names::PAGE_ENTRY_CAP, entry_grant: names::PAGE_ENTRY_GRANT,
    required_units: names::PAGE_REQUIRED_UNITS, keep_backups: true,
};
pub const INDEXES: Policy = Policy {
    folder: "/Index", kind: NameKind::Index, quota: names::INDEX_QUOTA,
    entry_cap: names::INDEX_ENTRY_CAP, entry_grant: names::INDEX_ENTRY_GRANT,
    required_units: names::INDEX_REQUIRED_UNITS, keep_backups: false,
};

pub struct Request<'a> {
    pub policy: &'a Policy,
    pub root: Option<&'a str>,
    pub cancel: &'a dyn Authority,
    pub clock: Option<fn() -> u32>,
    pub wait: Option<fn()>,
    pub pinned: Option<&'a [u8]>,
    pub need: Option<&'a PageCacheNeed>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant { pub units: u8, pub entries: u8, pub outcome: MissingProbe }

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
/// `Page` is a canonical file of the policy's own kind (a page or an index).
enum Rank { #[default] None, Page, Legacy, Temporary }
#[derive(Clone, Copy, Default)]
struct Victim { name: u64, bytes: u64, time: i64, rank: Rank }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass { Names, Sizes }
const VICTIMS: usize = 8;
struct Census<'a, 'folder> {
    request: Request<'a>,
    cadence: YieldCadence,
    folder: Option<&'folder core::ffi::CStr>,
    path: [u8; 400],
    victim: [Victim; VICTIMS],
    backups: PageNameSet,
    total: u64, count: u32, deleted: u32,
    victims: usize, met: usize, entries: u8,
    pass: Pass, valid: bool, outcome: MissingProbe,
}
impl<'a, 'folder> Census<'a, 'folder> {
    fn new(request: Request<'a>, folder: Option<&'folder core::ffi::CStr>) -> Self {
        Self { request, cadence: YieldCadence::new(), folder, path: [0;400],
            victim: [Victim::default();VICTIMS], backups: PageNameSet::default(), total:0, count:0, deleted:0,
            victims:0, met:0, entries:0, pass:Pass::Names, valid:folder.is_some(), outcome:MissingProbe::Ready }
    }
    fn result(&mut self, good: bool) -> bool {
        if !good && self.outcome == MissingProbe::Ready { self.outcome = MissingProbe::Denied; }
        good
    }
    fn poll(&mut self) -> bool {
        if self.outcome != MissingProbe::Ready { return false; }
        if self.request.cancel.cancelled() { self.outcome = MissingProbe::Stopped; return false; }
        if let (Some(clock),Some(wait)) = (self.request.clock,self.request.wait) {
            self.cadence.service(clock(),wait,clock);
        }
        let good = !self.request.cancel.cancelled();
        if !good { self.outcome = MissingProbe::Stopped; }
        good
    }
    fn grant<N: PageEntries>(&mut self, native: &mut N) -> u8 {
        if !self.valid || !self.poll() { return 0; }
        loop {
            if !self.scan(native) { return 0; }
            if self.victims != 0 { continue; }
            let policy = self.request.policy;
            let free = policy.quota.saturating_sub(self.total);
            let units = free/u64::from(names::PAGE_CREDIT_UNIT);
            self.entries = if self.count < u32::from(policy.entry_grant) {
                policy.entry_grant-self.count as u8
            } else { 0 };
            if units < u64::from(policy.required_units) { self.entries = 0; }
            return if self.entries != 0 { units.min(127) as u8 } else { 0 };
        }
    }
    fn scan<N: PageEntries>(&mut self, native: &mut N) -> bool {
        self.total=0;self.count=0;self.met=0;self.backups=PageNameSet::default();
        if !self.poll() { return false; }
        let Some(folder) = self.folder else { return self.result(false); };
        let Ok(folder_text)=folder.to_str() else { return self.result(false); };
        let found=native.stat(folder_text,true);
        let good=matches!(found,Ok(stat) if stat.kind==Kind::Directory) || matches!(found,Err(Error::Missing));
        if !self.result(good) || !self.poll() { return false; }
        if found.is_err() { return self.result(self.victims==0); }
        self.pass=Pass::Names;
        let named=self.read_entries(native,self.victims!=0);
        if !self.result(named) || !self.poll() { return false; }
        self.pass=Pass::Sizes;
        let sized=self.read_entries(native,true);
        self.result(sized) && self.poll()
    }
    fn make_path(&mut self, name: &[u8]) -> Option<&str> {
        let folder=self.folder?.to_bytes();
        let folder_bytes=folder.len();
        let bytes=folder_bytes.checked_add(1)?.checked_add(name.len())?;
        if bytes>=self.path.len() { return None; }
        self.path[..folder_bytes].copy_from_slice(folder);
        self.path[folder_bytes]=b'/';
        self.path[folder_bytes+1..bytes].copy_from_slice(name);
        core::str::from_utf8(&self.path[..bytes]).ok()
    }
    fn read_entries<N: PageEntries>(&mut self,native:&mut N,sized:bool)->bool {
        let Some(folder)=self.folder else { return false; };
        native.walk_entries(folder,sized,&mut |operation,entry| self.visit(operation,entry))
    }
    fn visit<N: Native>(&mut self,native:&mut N,entry:Entry<'_>)->bool {
        if !self.poll() { return false; }
        let Some(name)=entry.name else {
            if self.pass==Pass::Sizes { return self.sizes_done(); }
            if !self.result(self.met==self.victims) { return false; }
            self.victims=0;return true;
        };
        let parsed=PageName::parse(Some(name));
        if parsed.kind==NameKind::Unknown { return false; }
        if self.pass==Pass::Names {
            if let Some(index)=self.pending(name) { return self.remove(native,&entry,index); }
            if parsed.form==Form::Backup { self.backups.add(&name[..parsed.canonical_bytes]); }
            return true;
        }
        let good=entry.regular && names::add(&mut self.total,entry.bytes);
        if !self.result(good) { return false; }
        self.count=self.count.wrapping_add(1);
        self.consider(&entry,parsed);
        true
    }
    fn sizes_done(&mut self)->bool {
        let (mut count,mut total,mut keep)=(self.count,self.total,0);
        let need=self.request.need.map_or(1,|n|n.files);
        let policy=self.request.policy;
        while keep<self.victims {
            let rank=self.victim[keep].rank;
            if rank!=self.victim[0].rank { break; }
            let crowded=count.wrapping_add(u32::from(need))>u32::from(policy.entry_grant)
                || total>policy.quota || u64::from(policy.required_units)*u64::from(names::PAGE_CREDIT_UNIT)>policy.quota-total;
            if (rank!=Rank::Temporary && !crowded) || (rank==Rank::Page && count>policy.entry_cap) { break; }
            count=count.wrapping_sub(1);total=total.wrapping_sub(self.victim[keep].bytes);keep+=1;
        }
        self.victims=keep;true
    }
    fn pending(&self,name:&[u8])->Option<usize> {
        let hash=name_hash(name);
        (0..self.victims).find(|&i|self.victim[i].rank!=Rank::None && self.victim[i].name==hash)
    }
    fn remove<N: Native>(&mut self,native:&mut N,entry:&Entry<'_>,index:usize)->bool {
        let Some(name)=entry.name else { return self.result(false); };
        let victim=self.victim[index];
        let good=entry.regular && entry.bytes==victim.bytes && entry.time==victim.time
            && self.make_path(name).is_some() && self.deleted<self.request.policy.entry_cap;
        if !self.result(good) || !self.poll() { return false; }
        let Some(path)=self.make_path(name) else { return self.result(false); };
        let removed=native.unlink(path).is_ok();
        if !self.result(removed) { return false; }
        self.victim[index].rank=Rank::None;self.met+=1;self.deleted=self.deleted.wrapping_add(1);
        self.poll()
    }
    fn consider(&mut self,entry:&Entry<'_>,parsed:PageName) {
        let Some(name)=entry.name else { return; };
        let keep_backups=self.request.policy.keep_backups;
        let canonical=&name[..parsed.canonical_bytes];
        let page=parsed.kind==self.request.policy.kind;
        let rank=if page && parsed.form==Form::Temporary { Rank::Temporary }
            else if !page { Rank::Legacy }
            else if parsed.form==Form::Canonical || (parsed.form==Form::Backup && !keep_backups) { Rank::Page }
            else { Rank::None };
        if rank==Rank::None || (parsed.form==Form::Canonical && keep_backups && self.backups.may_contain(canonical))
            || (rank==Rank::Page && self.request.pinned==Some(canonical))
            || (rank==Rank::Page && self.request.need.is_some_and(|n|n.keep.may_contain(&name[65..parsed.canonical_bytes]))) { return; }
        let candidate=Victim{name:name_hash(name),bytes:entry.bytes,time:entry.time,rank};
        let mut at=self.victims;
        while at!=0 && before(&candidate,&self.victim[at-1]) { at-=1; }
        if at==VICTIMS { return; }
        if self.victims<VICTIMS { self.victims+=1; }
        for i in (at+1..self.victims).rev() { self.victim[i]=self.victim[i-1]; }
        self.victim[at]=candidate;
    }
}
fn name_hash(name:&[u8])->u64 {
    name.iter().fold(14695981039346656037,|v,b|(v^u64::from(*b)).wrapping_mul(1099511628211))
}
fn before(a:&Victim,b:&Victim)->bool {
    if a.rank!=b.rank { a.rank>b.rank }
    else if a.rank!=Rank::Temporary && a.time!=b.time { a.time<b.time }
    else { a.name<b.name }
}
/// A complete scan with no selected victims is the only grant authority.
pub fn entry_census<N: PageEntries>(native:&mut N,request:Request<'_>)->Grant {
    // The same frozen 192-byte folder backing, outside the mutably visited state.
    let mut folder=[0;192];
    let leaf=request.policy.folder.as_bytes();
    let length=request.root.and_then(|root| root.len().checked_add(leaf.len()));
    let folder=if let (Some(root),Some(bytes))=(request.root,length.filter(|&n| n<folder.len())) {
        folder[..root.len()].copy_from_slice(root.as_bytes());
        folder[root.len()..bytes].copy_from_slice(leaf);
        core::ffi::CStr::from_bytes_with_nul(&folder[..=bytes]).ok()
    } else { None };
    let mut census=Census::new(request,folder);
    let units=census.grant(native);
    let outcome=if units>=census.request.policy.required_units { MissingProbe::Ready }
        else if census.outcome==MissingProbe::Stopped { MissingProbe::Stopped } else { MissingProbe::Denied };
    Grant{units,entries:if units!=0 { census.entries } else {0},outcome}
}
