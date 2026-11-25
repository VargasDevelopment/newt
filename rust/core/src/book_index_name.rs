//! One FAT directory entry for every kind of index file. The lossy filename
//! never grants identity; the envelope and index header bind the full digest.
use super::Digest;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind { Index, Temporary, Work, Backup }
impl FileKind {
    fn extension(self) -> &'static [u8;3] {
        match self { Self::Index=>b"IDX",Self::Temporary=>b"TMP",Self::Work=>b"WRK",Self::Backup=>b"BAK" }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileName([u8;12]);
impl FileName {
    pub fn new(digest:&Digest,kind:FileKind)->Self {
        const ALPHABET:&[u8;32]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let prefix=digest[..5].iter().fold(0u64,|value,&byte|(value<<8)|u64::from(byte));
        let mut bytes=[0;12];
        for (i,byte) in bytes[..8].iter_mut().enumerate() {
            *byte=ALPHABET[((prefix>>(35-5*i))&31) as usize];
        }
        bytes[8]=b'.';bytes[9..].copy_from_slice(kind.extension());
        Self(bytes)
    }
    pub fn parse(bytes:&[u8])->Option<Self> {
        let bytes:[u8;12]=bytes.try_into().ok()?;
        if bytes[8]!=b'.' || !bytes[..8].iter().all(|b|b.is_ascii_uppercase()||(b'2'..=b'7').contains(b)) {
            return None;
        }
        if !matches!(&bytes[9..],b"IDX"|b"TMP"|b"WRK"|b"BAK") {return None;}
        Some(Self(bytes))
    }
    pub fn kind(&self)->FileKind {
        match &self.0[9..] {b"IDX"=>FileKind::Index,b"TMP"=>FileKind::Temporary,b"WRK"=>FileKind::Work,_=>FileKind::Backup}
    }
    pub fn with_kind(mut self,kind:FileKind)->Self {
        self.0[9..].copy_from_slice(kind.extension());self
    }
    pub fn as_bytes(&self)->&[u8;12] {&self.0}
    pub fn as_str(&self)->&str {
        // Both constructors admit only the ASCII FAT alphabet above.
        core::str::from_utf8(&self.0).expect("validated index filename")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base32_uses_exactly_the_first_forty_digest_bits() {
        assert_eq!(FileName::new(&[0;32],FileKind::Index).as_bytes(),b"AAAAAAAA.IDX");
        assert_eq!(FileName::new(&[255;32],FileKind::Index).as_bytes(),b"77777777.IDX");
        let mut digest=[0;32];digest[..5].copy_from_slice(&[0,1,2,3,4]);
        assert_eq!(FileName::new(&digest,FileKind::Index).as_bytes(),b"AAAQEAYE.IDX");
        let original=FileName::new(&digest,FileKind::Index);
        digest[31]^=1;
        assert_eq!(FileName::new(&digest,FileKind::Index),original,"filename collisions are intentional, identity is not the filename");
    }
    #[test]
    fn every_file_kind_is_one_uppercase_eight_dot_three_name() {
        let canonical=FileName::new(&[37;32],FileKind::Index);
        for kind in [FileKind::Index,FileKind::Temporary,FileKind::Work,FileKind::Backup] {
            let name=canonical.with_kind(kind);
            assert_eq!(name.kind(),kind);
            assert_eq!(FileName::parse(name.as_bytes()),Some(name));
            assert_eq!(name.as_str().len(),12);
            assert_eq!(&name.as_bytes()[..8],&canonical.as_bytes()[..8]);
        }
    }
    #[test]
    fn malformed_and_long_names_are_not_short_index_files() {
        for bytes in [b"AAAAAAAA.idx".as_slice(),b"aaaaAAAA.IDX",b"AAAAAAA1.IDX",b"AAAAAAA8.IDX",b"AAAAAAA/.IDX",b"AAAAAAA.IDX",b"AAAAAAAA.IDX.bak",b"AAAAAAAA.00000000.tmp",b"AAAAAAAA.TXT"] {
            assert_eq!(FileName::parse(bytes),None);
        }
    }
}
