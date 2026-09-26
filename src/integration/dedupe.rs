//! Read-only duplicate-file scan: walk, stat, hash, and query APFS private size.
//!
//! All I/O for the `dedupe` capability lives here; grouping and reclaim math
//! live in [`crate::domain::dedupe`]. Nothing in this module modifies the
//! filesystem. Clone replacement (execute) is UNSUPPORTED — see
//! [`crate::domain::dedupe::EXECUTE_STATUS`].

use std::{
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

use anyhow::Context as _;
use rayon::prelude::*;

use crate::domain::dedupe::{
    build_report, size_collision_candidates, DedupeReport, DupFile, FileStat,
};

/// Walks `root` (no symlink following, hidden files included) and returns a
/// [`FileStat`] for every regular file. Symlinks, directories, and special
/// files are skipped.
pub fn walk_regular_files(root: &Path) -> anyhow::Result<Vec<FileStat>> {
    use jwalk::{Parallelism, WalkDir};

    std::fs::symlink_metadata(root)
        .with_context(|| format!("Cannot stat dedupe root: {}", root.display()))?;

    let files = WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(Parallelism::RayonNewPool(0))
        .into_iter()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.file_type().is_file() {
                return None;
            }
            let meta = entry.metadata().ok()?;
            if !meta.file_type().is_file() {
                return None;
            }
            Some(FileStat {
                path: entry.path(),
                size: meta.len(),
                dev: meta.dev(),
                ino: meta.ino(),
            })
        })
        .collect();
    Ok(files)
}

/// Returns the APFS private (unshared) allocated size of `path` via
/// `getattrlist(ATTR_CMNEXT_PRIVATESIZE)`, or `None` if the volume/OS does
/// not return that attribute (non-APFS, pre-10.15) or the call fails.
///
/// ```
/// use osx_clnr::integration::dedupe::private_size;
/// use std::io::Write;
///
/// // Positive: a freshly written file on APFS owns all of its blocks.
/// let dir = tempfile::tempdir().unwrap();
/// let p = dir.path().join("f");
/// std::fs::File::create(&p).unwrap().write_all(&vec![7u8; 128 * 1024]).unwrap();
/// if let Some(sz) = private_size(&p) {
///     assert!(sz >= 128 * 1024);
/// }
///
/// // Refusal: a missing path yields None, not a panic.
/// assert_eq!(private_size(std::path::Path::new("/nonexistent/definitely/not/here")), None);
/// ```
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // audited: libc::getattrlist FFI, fixed-size output buffer, bounds-checked read
pub fn private_size(path: &Path) -> Option<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt as _};

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut req = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: libc::ATTR_CMNEXT_PRIVATESIZE,
    };
    // Layout: u32 length | attribute_set_t (5 x u32) | off_t private size.
    let mut buf = [0u8; 64];
    let rc = unsafe {
        libc::getattrlist(
            c_path.as_ptr(),
            (&mut req as *mut libc::attrlist).cast(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            libc::FSOPT_NOFOLLOW | libc::FSOPT_ATTR_CMN_EXTENDED,
        )
    };
    if rc != 0 {
        return None;
    }
    let u32_at = |off: usize| u32::from_ne_bytes(buf[off..off + 4].try_into().unwrap_or([0; 4]));
    let length = u32_at(0) as usize;
    // attribute_set_t: commonattr@4, volattr@8, dirattr@12, fileattr@16, forkattr@20.
    let returned_fork = u32_at(20);
    if returned_fork & libc::ATTR_CMNEXT_PRIVATESIZE == 0 || length < 32 {
        return None;
    }
    let raw = i64::from_ne_bytes(buf[24..32].try_into().ok()?);
    u64::try_from(raw).ok()
}

/// Non-macOS fallback: the attribute does not exist.
#[cfg(not(target_os = "macos"))]
pub fn private_size(_path: &Path) -> Option<u64> {
    None
}

/// Runs the full read-only dedupe scan over `roots`.
///
/// Walk → size-collision filter (domain) → parallel BLAKE3 hash + private
/// size (integration) → grouping and reclaim (domain).
pub fn scan_duplicates(roots: &[PathBuf], min_size: u64) -> anyhow::Result<DedupeReport> {
    let mut all = Vec::new();
    for root in roots {
        all.extend(walk_regular_files(root)?);
    }
    let files_scanned = all.len() as u64;
    let candidates = size_collision_candidates(all, min_size);

    let results: Vec<Option<DupFile>> = candidates
        .into_par_iter()
        .map(|c| {
            let hash = crate::integration::fs::generate_manifest(&c.path).ok()?;
            Some(DupFile {
                private_size: private_size(&c.path),
                path: c.path,
                size: c.size,
                dev: c.dev,
                hash,
            })
        })
        .collect();
    let hash_errors = results.iter().filter(|r| r.is_none()).count() as u64;
    let hashed: Vec<DupFile> = results.into_iter().flatten().collect();

    Ok(build_report(roots.to_vec(), min_size, files_scanned, hashed, hash_errors))
}
