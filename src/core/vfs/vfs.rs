//! SQLite VFS implementation
//!
//! Implements the sqlite3_vfs interface to provide filesystem operations
//! backed by Cartridge archive storage.
//!
//! Supports multiple simultaneous cartridge VFS instances via unique names.
//! Each cartridge registers its own VFS (`cartridge-{id}`) so multiple
//! cartridges can have SQLite databases open at the same time.

use super::super::cartridge::Cartridge;
use crate::error::{CartridgeError, Result};
use libsqlite3_sys as ffi;
use parking_lot::Mutex;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Default VFS name (for backwards compatibility with single-cartridge usage)
pub const VFS_NAME: &str = "cartridge";

/// Global counter for generating unique VFS names
static VFS_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Cartridge VFS instance
pub struct CartridgeVFS {
    /// Underlying cartridge archive
    cartridge: Arc<Mutex<Cartridge>>,
    /// VFS name (C string) — unique per instance
    name: CString,
}

impl CartridgeVFS {
    /// Create a new VFS with the default name (single-cartridge compat)
    pub fn new(cartridge: Arc<Mutex<Cartridge>>) -> Result<Self> {
        Self::with_name(VFS_NAME, cartridge)
    }

    /// Create a new VFS with a specific name
    pub fn with_name(name: &str, cartridge: Arc<Mutex<Cartridge>>) -> Result<Self> {
        let name = CString::new(name)
            .map_err(|e| CartridgeError::Allocation(format!("Invalid VFS name: {}", e)))?;

        Ok(Self { cartridge, name })
    }

    /// Get the cartridge
    pub fn cartridge(&self) -> &Arc<Mutex<Cartridge>> {
        &self.cartridge
    }

    /// Get the VFS name as a string
    pub fn name_str(&self) -> &str {
        self.name.to_str().unwrap_or(VFS_NAME)
    }
}

/// Generate a unique VFS name for a cartridge instance.
pub fn generate_vfs_name() -> String {
    let id = VFS_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("cartridge-{id}")
}

/// Register a cartridge VFS with the default name.
/// For single-cartridge usage or backwards compatibility.
pub fn register_vfs(cartridge: Arc<Mutex<Cartridge>>) -> Result<()> {
    register_named_vfs(VFS_NAME, cartridge)
}

/// Register a cartridge VFS with a specific name.
/// Use this for multi-cartridge scenarios where each cartridge needs its own VFS.
///
/// Returns the VFS name that was registered (same as the `name` parameter).
/// Use this name in SQLite URI: `file:db.db?vfs={name}`
pub fn register_named_vfs(name: &str, cartridge: Arc<Mutex<Cartridge>>) -> Result<()> {
    let vfs = CartridgeVFS::with_name(name, cartridge)?;
    let vfs_ptr = Box::into_raw(Box::new(vfs));

    // Create the sqlite3_vfs structure
    let sqlite_vfs = Box::new(ffi::sqlite3_vfs {
        iVersion: 3,
        szOsFile: std::mem::size_of::<super::file::CartridgeFile>() as c_int,
        mxPathname: 1024,
        pNext: ptr::null_mut(),
        zName: (*unsafe { &*vfs_ptr }).name.as_ptr(),
        pAppData: vfs_ptr as *mut c_void,
        xOpen: Some(vfs_open),
        xDelete: Some(vfs_delete),
        xAccess: Some(vfs_access),
        xFullPathname: Some(vfs_full_pathname),
        xDlOpen: None,
        xDlError: None,
        xDlSym: None,
        xDlClose: None,
        xRandomness: Some(vfs_randomness),
        xSleep: Some(vfs_sleep),
        xCurrentTime: Some(vfs_current_time),
        xGetLastError: Some(vfs_get_last_error),
        xCurrentTimeInt64: Some(vfs_current_time_int64),
        xSetSystemCall: None,
        xGetSystemCall: None,
        xNextSystemCall: None,
    });

    let vfs_box_ptr = Box::into_raw(sqlite_vfs);

    unsafe {
        let rc = ffi::sqlite3_vfs_register(vfs_box_ptr, 0);
        if rc != ffi::SQLITE_OK {
            // Clean up on failure
            drop(Box::from_raw(vfs_box_ptr));
            drop(Box::from_raw(vfs_ptr));
            return Err(CartridgeError::VFSRegistrationFailed(rc));
        }
    }

    Ok(())
}

/// Unregister the default-named Cartridge VFS from SQLite.
pub fn unregister_vfs() -> Result<()> {
    unregister_named_vfs(VFS_NAME)
}

/// Unregister a named Cartridge VFS from SQLite.
pub fn unregister_named_vfs(name: &str) -> Result<()> {
    let cname = CString::new(name)
        .map_err(|e| CartridgeError::Allocation(format!("Invalid VFS name: {}", e)))?;

    unsafe {
        let vfs_ptr = ffi::sqlite3_vfs_find(cname.as_ptr());
        if vfs_ptr.is_null() {
            return Ok(()); // Already unregistered
        }

        let rc = ffi::sqlite3_vfs_unregister(vfs_ptr);
        if rc != ffi::SQLITE_OK {
            return Err(CartridgeError::VFSRegistrationFailed(rc));
        }

        // Clean up allocated memory
        let app_data = (*vfs_ptr).pAppData;
        if !app_data.is_null() {
            drop(Box::from_raw(app_data as *mut CartridgeVFS));
        }
        drop(Box::from_raw(vfs_ptr as *mut ffi::sqlite3_vfs));
    }

    Ok(())
}

// VFS callback functions

unsafe extern "C" fn vfs_open(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    p_out_flags: *mut c_int,
) -> c_int {
    super::file::file_open(vfs, z_name, file, flags, p_out_flags)
}

unsafe extern "C" fn vfs_delete(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    _sync_dir: c_int,
) -> c_int {
    let app_data = (*vfs).pAppData as *mut CartridgeVFS;
    if app_data.is_null() {
        return ffi::SQLITE_ERROR;
    }

    let vfs_impl = &*app_data;
    let path = match CStr::from_ptr(z_name).to_str() {
        Ok(p) => p,
        Err(_) => return ffi::SQLITE_ERROR,
    };

    let mut cartridge = vfs_impl.cartridge.lock();
    match cartridge.delete_file(path) {
        Ok(_) => ffi::SQLITE_OK,
        Err(_) => ffi::SQLITE_IOERR_DELETE,
    }
}

unsafe extern "C" fn vfs_access(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    _flags: c_int,
    p_res_out: *mut c_int,
) -> c_int {
    let app_data = (*vfs).pAppData as *mut CartridgeVFS;
    if app_data.is_null() {
        return ffi::SQLITE_ERROR;
    }

    let vfs_impl = &*app_data;
    let path = match CStr::from_ptr(z_name).to_str() {
        Ok(p) => p,
        Err(_) => return ffi::SQLITE_ERROR,
    };

    let cartridge = vfs_impl.cartridge.lock();
    let exists = cartridge.exists(path).unwrap_or(false);

    *p_res_out = if exists { 1 } else { 0 };
    ffi::SQLITE_OK
}

unsafe extern "C" fn vfs_full_pathname(
    _vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    n_out: c_int,
    z_out: *mut c_char,
) -> c_int {
    // Just copy the input path as-is (paths in cartridge are already absolute within the archive)
    let len = libc::strlen(z_name);
    if len >= n_out as usize {
        return ffi::SQLITE_CANTOPEN;
    }

    libc::strcpy(z_out, z_name);
    ffi::SQLITE_OK
}

unsafe extern "C" fn vfs_randomness(
    _vfs: *mut ffi::sqlite3_vfs,
    n_byte: c_int,
    z_out: *mut c_char,
) -> c_int {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Fill with pseudo-random data based on timestamp
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let bytes = z_out as *mut u8;
    for i in 0..n_byte as usize {
        *bytes.add(i) = ((nanos >> (i * 8)) & 0xFF) as u8;
    }

    n_byte
}

unsafe extern "C" fn vfs_sleep(_vfs: *mut ffi::sqlite3_vfs, microseconds: c_int) -> c_int {
    std::thread::sleep(std::time::Duration::from_micros(microseconds as u64));
    microseconds
}

unsafe extern "C" fn vfs_current_time(_vfs: *mut ffi::sqlite3_vfs, p_time_out: *mut f64) -> c_int {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    // Julian day number (days since noon UTC on November 24, 4714 BC)
    let julian_day = 2440587.5 + (duration.as_secs_f64() / 86400.0);

    *p_time_out = julian_day;
    ffi::SQLITE_OK
}

unsafe extern "C" fn vfs_current_time_int64(
    _vfs: *mut ffi::sqlite3_vfs,
    p_time_out: *mut ffi::sqlite3_int64,
) -> c_int {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

    // Julian day in milliseconds
    let julian_ms = 210866760000000i64 + (duration.as_millis() as i64);

    *p_time_out = julian_ms;
    ffi::SQLITE_OK
}

unsafe extern "C" fn vfs_get_last_error(
    _vfs: *mut ffi::sqlite3_vfs,
    _n_byte: c_int,
    _z_err_msg: *mut c_char,
) -> c_int {
    // No specific error tracking for now
    0
}
