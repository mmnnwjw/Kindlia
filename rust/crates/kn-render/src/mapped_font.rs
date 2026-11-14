//! Read-only demand paging for immutable Kindle firmware fonts only.
//! User fonts remain owned: users can overwrite them while the reader is running.
use std::{fs::File, io, os::fd::AsRawFd, path::Path, ptr::NonNull};
pub(crate) struct MappedFont {
    ptr: NonNull<u8>,
    len: usize,
}
// The mapping is read-only and firmware font files are never modified by this app.
unsafe impl Send for MappedFont {}
unsafe impl Sync for MappedFont {}
impl MappedFont {
    pub fn open(path: &Path) -> io::Result<Self> {
        let resolved = path.canonicalize()?;
        if !resolved.starts_with("/usr/java/lib/fonts") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an immutable system font",
            ));
        }
        let file = File::open(resolved)?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| io::Error::other("font too large"))?;
        if len == 0 {
            return Err(io::Error::other("empty font"));
        }
        // SAFETY: fd is live for mmap; the private, read-only mapping outlives the fd.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let Some(ptr) = NonNull::new(p.cast()) else {
            // A null-address mapping cannot be represented by a Rust slice.
            unsafe {
                libc::munmap(p, len);
            }
            return Err(io::Error::other("null mapping"));
        };
        Ok(Self { ptr, len })
    }
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: mmap owns exactly len readable bytes until Drop; no mutable view exists.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}
impl Drop for MappedFont {
    fn drop(&mut self) {
        // SAFETY: this is the original mapping/length, with no surviving borrowed slices.
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast(), self.len);
        }
    }
}
