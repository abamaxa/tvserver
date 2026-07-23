use sha2::{Digest, Sha256};
use std::io::{self, Read};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSeal {
    pub len: u64,
    pub sha256: [u8; 32],
}

pub fn seal_reader<R: Read>(reader: &mut R) -> io::Result<FileSeal> {
    let mut hasher = Sha256::new();
    let mut len = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        len += read as u64;
    }
    Ok(FileSeal {
        len,
        sha256: hasher.finalize().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_changes_when_same_length_content_changes() {
        let first = seal_reader(&mut &b"first"[..]).unwrap();
        let second = seal_reader(&mut &b"other"[..]).unwrap();
        assert_eq!(first.len, second.len);
        assert_ne!(first.sha256, second.sha256);
    }

    #[test]
    fn seal_covers_bytes_after_database_checksum_window() {
        let first = vec![0_u8; 12 * 1024 * 1024];
        let mut second = first.clone();
        second[11 * 1024 * 1024] = 1;
        assert_ne!(
            seal_reader(&mut first.as_slice()).unwrap(),
            seal_reader(&mut second.as_slice()).unwrap()
        );
    }
}
