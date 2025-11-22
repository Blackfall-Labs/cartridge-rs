//! SQLite file I/O methods implementation
//!
//! Implements sqlite3_io_methods for reading and writing database files
//! within a Cartridge archive.

use super::vfs::CartridgeVFS;
use libsqlite3_sys as ffi;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

/// Cartridge-backed SQLite file
#[repr(C)]
pub struct CartridgeFile {
    /// Base sqlite3_file structure (MUST be first field)
    pub base: ffi::sqlite3_file,
    /// Path to the file within the cartridge
    pub path: String,
    /// Reference to the VFS (for accessing the cartridge)
    pub vfs: *mut CartridgeVFS,
    /// Current file size
    pub size: u64,
    /// Lock state
    pub lock_level: c_int,
}

// File I/O method implementations

unsafe extern "C" fn file_close(file: *mut ffi::sqlite3_file) -> c_int {
    let cart_file = &mut *(file as *mut CartridgeFile);

    // Flush any pending writes
    let vfs = &*cart_file.vfs;
    let mut cartridge = vfs.cartridge().lock();

    match cartridge.flush() {
        Ok(_) => ffi::SQLITE_OK,
        Err(_) => ffi::SQLITE_IOERR_CLOSE,
    }
}

unsafe extern "C" fn file_read(
    file: *mut ffi::sqlite3_file,
    buf: *mut c_void,
    amt: c_int,
    offset: ffi::sqlite3_int64,
) -> c_int {
    let cart_file = &mut *(file as *mut CartridgeFile);
    let vfs = &*cart_file.vfs;
    let mut cartridge = vfs.cartridge().lock();

    // Check bounds
    if offset < 0 {
        return ffi::SQLITE_IOERR_READ;
    }

    let offset = offset as u64;
    let amt = amt as usize;

    // Read the file
    let data = match cartridge.read_file(&cart_file.path) {
        Ok(d) => d,
        Err(_) => {
            // File doesn't exist yet - fill with zeros (SQLite expects this)
            let zeros = vec![0u8; amt];
            std::ptr::copy_nonoverlapping(zeros.as_ptr(), buf as *mut u8, amt);
            return if amt > 0 {
                ffi::SQLITE_IOERR_SHORT_READ
            } else {
                ffi::SQLITE_OK
            };
        }
    };

    // Check if we're reading past the end of the file
    if offset >= data.len() as u64 {
        // Reading past EOF - fill with zeros
        std::ptr::write_bytes(buf as *mut u8, 0, amt);
        return ffi::SQLITE_IOERR_SHORT_READ;
    }

    let available = (data.len() as u64 - offset) as usize;
    let to_read = amt.min(available);

    // Copy data to buffer
    std::ptr::copy_nonoverlapping(data.as_ptr().add(offset as usize), buf as *mut u8, to_read);

    // Fill remainder with zeros if short read
    if to_read < amt {
        std::ptr::write_bytes((buf as *mut u8).add(to_read), 0, amt - to_read);
        ffi::SQLITE_IOERR_SHORT_READ
    } else {
        ffi::SQLITE_OK
    }
}

unsafe extern "C" fn file_write(
    file: *mut ffi::sqlite3_file,
    buf: *const c_void,
    amt: c_int,
    offset: ffi::sqlite3_int64,
) -> c_int {
    let cart_file = &mut *(file as *mut CartridgeFile);
    let vfs = &*cart_file.vfs;
    let mut cartridge = vfs.cartridge().lock();

    if offset < 0 {
        return ffi::SQLITE_IOERR_WRITE;
    }

    let offset = offset as u64;
    let amt = amt as usize;
    let data_slice = std::slice::from_raw_parts(buf as *const u8, amt);

    // Read existing file content
    let mut content = cartridge
        .read_file(&cart_file.path)
        .unwrap_or_else(|_| Vec::new());

    // Extend file if necessary
    let end_pos = offset + amt as u64;
    if content.len() < end_pos as usize {
        content.resize(end_pos as usize, 0);
    }

    // Write new data
    content[offset as usize..end_pos as usize].copy_from_slice(data_slice);

    // Write back to cartridge
    match cartridge.write_file(&cart_file.path, &content) {
        Ok(_) => {
            cart_file.size = content.len() as u64;
            ffi::SQLITE_OK
        }
        Err(_) => ffi::SQLITE_IOERR_WRITE,
    }
}

unsafe extern "C" fn file_truncate(
    file: *mut ffi::sqlite3_file,
    size: ffi::sqlite3_int64,
) -> c_int {
    let cart_file = &mut *(file as *mut CartridgeFile);
    let vfs = &*cart_file.vfs;
    let mut cartridge = vfs.cartridge().lock();

    if size < 0 {
        return ffi::SQLITE_IOERR_TRUNCATE;
    }

    let new_size = size as usize;

    // Read existing content
    let mut content = cartridge
        .read_file(&cart_file.path)
        .unwrap_or_else(|_| Vec::new());

    // Resize
    content.resize(new_size, 0);

    // Write back
    match cartridge.write_file(&cart_file.path, &content) {
        Ok(_) => {
            cart_file.size = new_size as u64;
            ffi::SQLITE_OK
        }
        Err(_) => ffi::SQLITE_IOERR_TRUNCATE,
    }
}

unsafe extern "C" fn file_sync(file: *mut ffi::sqlite3_file, _flags: c_int) -> c_int {
    let cart_file = &*(file as *mut CartridgeFile);
    let vfs = &*cart_file.vfs;
    let mut cartridge = vfs.cartridge().lock();

    match cartridge.flush() {
        Ok(_) => ffi::SQLITE_OK,
        Err(_) => ffi::SQLITE_IOERR_FSYNC,
    }
}

unsafe extern "C" fn file_file_size(
    file: *mut ffi::sqlite3_file,
    p_size: *mut ffi::sqlite3_int64,
) -> c_int {
    let cart_file = &*(file as *mut CartridgeFile);
    let vfs = &*cart_file.vfs;
    let cartridge = vfs.cartridge().lock();

    match cartridge.metadata(&cart_file.path) {
        Ok(meta) => {
            *p_size = meta.size as ffi::sqlite3_int64;
            ffi::SQLITE_OK
        }
        Err(_) => {
            *p_size = 0;
            ffi::SQLITE_OK
        }
    }
}

unsafe extern "C" fn file_lock(file: *mut ffi::sqlite3_file, lock_type: c_int) -> c_int {
    let cart_file = &mut *(file as *mut CartridgeFile);

    // Simple lock implementation (single-user for now)
    if lock_type > cart_file.lock_level {
        cart_file.lock_level = lock_type;
        ffi::SQLITE_OK
    } else {
        ffi::SQLITE_OK
    }
}

unsafe extern "C" fn file_unlock(file: *mut ffi::sqlite3_file, lock_type: c_int) -> c_int {
    let cart_file = &mut *(file as *mut CartridgeFile);

    if lock_type < cart_file.lock_level {
        cart_file.lock_level = lock_type;
    }
    ffi::SQLITE_OK
}

unsafe extern "C" fn file_check_reserved_lock(
    file: *mut ffi::sqlite3_file,
    p_res_out: *mut c_int,
) -> c_int {
    let cart_file = &*(file as *mut CartridgeFile);

    *p_res_out = if cart_file.lock_level >= ffi::SQLITE_LOCK_RESERVED {
        1
    } else {
        0
    };
    ffi::SQLITE_OK
}

unsafe extern "C" fn file_file_control(
    _file: *mut ffi::sqlite3_file,
    _op: c_int,
    _p_arg: *mut c_void,
) -> c_int {
    // Return NOTFOUND for unhandled operations
    ffi::SQLITE_NOTFOUND
}

unsafe extern "C" fn file_sector_size(_file: *mut ffi::sqlite3_file) -> c_int {
    // Return 4KB sector size (matches our PAGE_SIZE)
    4096
}

unsafe extern "C" fn file_device_characteristics(_file: *mut ffi::sqlite3_file) -> c_int {
    // Indicate that our storage is safe for atomic writes
    ffi::SQLITE_IOCAP_ATOMIC4K | ffi::SQLITE_IOCAP_SAFE_APPEND
}

// VFS open callback

pub unsafe extern "C" fn file_open(
    vfs: *mut ffi::sqlite3_vfs,
    z_name: *const c_char,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    p_out_flags: *mut c_int,
) -> c_int {
    let app_data = (*vfs).pAppData as *mut CartridgeVFS;
    if app_data.is_null() {
        return ffi::SQLITE_ERROR;
    }

    let vfs_impl = &mut *app_data;

    // Get path
    let path = if z_name.is_null() {
        // Temp file - generate a unique name
        format!("temp_{}", uuid())
    } else {
        match CStr::from_ptr(z_name).to_str() {
            Ok(p) => p.to_string(),
            Err(_) => return ffi::SQLITE_ERROR,
        }
    };

    // Initialize the CartridgeFile structure
    let cart_file = &mut *(file as *mut CartridgeFile);
    cart_file.path = path.clone();
    cart_file.vfs = vfs_impl as *mut CartridgeVFS;
    cart_file.size = 0;
    cart_file.lock_level = ffi::SQLITE_LOCK_NONE;

    // Set up the io_methods
    let io_methods = Box::new(ffi::sqlite3_io_methods {
        iVersion: 1,
        xClose: Some(file_close),
        xRead: Some(file_read),
        xWrite: Some(file_write),
        xTruncate: Some(file_truncate),
        xSync: Some(file_sync),
        xFileSize: Some(file_file_size),
        xLock: Some(file_lock),
        xUnlock: Some(file_unlock),
        xCheckReservedLock: Some(file_check_reserved_lock),
        xFileControl: Some(file_file_control),
        xSectorSize: Some(file_sector_size),
        xDeviceCharacteristics: Some(file_device_characteristics),
        xShmMap: None,
        xShmLock: None,
        xShmBarrier: None,
        xShmUnmap: None,
        xFetch: None,
        xUnfetch: None,
    });

    cart_file.base.pMethods = Box::into_raw(io_methods);

    // Create the file if it doesn't exist (for CREATE flag)
    if flags & ffi::SQLITE_OPEN_CREATE != 0 {
        let mut cartridge = vfs_impl.cartridge().lock();
        if !cartridge.exists(&path).unwrap_or(false) {
            if let Err(_) = cartridge.create_file(&path, &[]) {
                return ffi::SQLITE_CANTOPEN;
            }
        }
    }

    // Set output flags
    if !p_out_flags.is_null() {
        *p_out_flags = flags;
    }

    ffi::SQLITE_OK
}

// Helper function for generating unique temp file names
fn uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", nanos)
}
