// SPDX-License-Identifier: MPL-2.0

#![allow(dead_code)]

use super::SyscallReturn;
use crate::{
    fs::{
        file_table::FileDesc,
        fs_resolver::{FsPath, AT_FDCWD},
        utils::Metadata,
    },
    prelude::*,
    syscall::constants::MAX_FILENAME_LEN,
    time::timespec_t,
    util::{read_cstring_from_user, write_val_to_user},
};

pub fn sys_fstat(fd: FileDesc, stat_buf_ptr: Vaddr) -> Result<SyscallReturn> {
    debug!("fd = {}, stat_buf_addr = 0x{:x}", fd, stat_buf_ptr);

    let current = current!();
    let file_table = current.file_table().lock();
    let file = file_table.get_file(fd)?;
    let stat = Stat::from(file.metadata());
    write_val_to_user(stat_buf_ptr, &stat)?;
    Ok(SyscallReturn::Return(0))
}

pub fn sys_stat(filename_ptr: Vaddr, stat_buf_ptr: Vaddr) -> Result<SyscallReturn> {
    self::sys_fstatat(AT_FDCWD, filename_ptr, stat_buf_ptr, 0)
}

pub fn sys_lstat(filename_ptr: Vaddr, stat_buf_ptr: Vaddr) -> Result<SyscallReturn> {
    self::sys_fstatat(
        AT_FDCWD,
        filename_ptr,
        stat_buf_ptr,
        StatFlags::AT_SYMLINK_NOFOLLOW.bits(),
    )
}

pub fn sys_fstatat(
    dirfd: FileDesc,
    filename_ptr: Vaddr,
    stat_buf_ptr: Vaddr,
    flags: u32,
) -> Result<SyscallReturn> {
    let filename = read_cstring_from_user(filename_ptr, MAX_FILENAME_LEN)?;
    let flags =
        StatFlags::from_bits(flags).ok_or(Error::with_message(Errno::EINVAL, "invalid flags"))?;
    debug!(
        "dirfd = {}, filename = {:?}, stat_buf_ptr = 0x{:x}, flags = {:?}",
        dirfd, filename, stat_buf_ptr, flags
    );

    if filename.is_empty() {
        if !flags.contains(StatFlags::AT_EMPTY_PATH) {
            return_errno_with_message!(Errno::ENOENT, "path is empty");
        }
        // In this case, the behavior of fstatat() is similar to that of fstat().
        return self::sys_fstat(dirfd, stat_buf_ptr);
    }

    let current = current!();
    let dentry = {
        let filename = filename.to_string_lossy();
        let fs_path = FsPath::new(dirfd, filename.as_ref())?;
        let fs = current.fs().read();
        if flags.contains(StatFlags::AT_SYMLINK_NOFOLLOW) {
            fs.lookup_no_follow(&fs_path)?
        } else {
            fs.lookup(&fs_path)?
        }
    };
    let stat = Stat::from(dentry.metadata());
    write_val_to_user(stat_buf_ptr, &stat)?;
    Ok(SyscallReturn::Return(0))
}

/// File type mask.
const S_IFMT: u16 = 0o170000;

/// Enum representing different file types.
#[derive(Debug, PartialEq)]
pub enum FileType {
    Socket,
    CharacterDevice,
    BlockDevice,
    Directory,
    Fifo,
    RegularFile,
    Symlink,
    Unknown,
}

impl FileType {
    /// Extract the file type from the mode.
    pub fn from_mode(mode: u16) -> FileType {
        match mode & S_IFMT {
            0o140000 => FileType::Socket,          // Socket.
            0o020000 => FileType::CharacterDevice, // Character device.
            0o060000 => FileType::BlockDevice,     // Block device.
            0o040000 => FileType::Directory,       // Directory.
            0o010000 => FileType::Fifo,            // FIFO (named pipe).
            0 | 0o100000 => FileType::RegularFile, // Regular file.
            0o120000 => FileType::Symlink,         // Symbolic link.
            _ => FileType::Unknown,                // Unkonwn file type.
        }
    }
}

/// File Stat
#[derive(Debug, Clone, Copy, Pod, Default)]
#[repr(C)]
pub struct Stat {
    /// ID of device containing file
    st_dev: u64,
    /// Inode number
    st_ino: u64,
    /// Number of hard links
    st_nlink: usize,
    /// File type and mode
    st_mode: u32,
    /// User ID of owner
    st_uid: u32,
    /// Group ID of owner
    st_gid: u32,
    /// Padding bytes
    __pad0: u32,
    /// Device ID (if special file)
    st_rdev: u64,
    /// Total size, in bytes
    st_size: isize,
    /// Block size for filesystem I/O
    st_blksize: isize,
    /// Number of 512-byte blocks allocated
    st_blocks: isize,
    /// Time of last access
    st_atime: timespec_t,
    /// Time of last modification
    st_mtime: timespec_t,
    /// Time of last status change
    st_ctime: timespec_t,
    /// Unused field
    __unused: [i64; 3],
}

impl From<Metadata> for Stat {
    fn from(info: Metadata) -> Self {
        Self {
            st_dev: info.dev,
            st_ino: info.ino,
            st_nlink: info.nlinks,
            st_mode: info.type_ as u32 | info.mode.bits() as u32,
            st_uid: info.uid.as_u32(),
            st_gid: info.gid.as_u32(),
            __pad0: 0,
            st_rdev: info.rdev,
            st_size: info.size as isize,
            st_blksize: info.blk_size as isize,
            st_blocks: (info.blocks * (info.blk_size / 512)) as isize, // Number of 512B blocks
            st_atime: info.atime.into(),
            st_mtime: info.mtime.into(),
            st_ctime: info.ctime.into(),
            __unused: [0; 3],
        }
    }
}

bitflags::bitflags! {
    struct StatFlags: u32 {
        const AT_EMPTY_PATH = 1 << 12;
        const AT_NO_AUTOMOUNT = 1 << 11;
        const AT_SYMLINK_NOFOLLOW = 1 << 8;
    }
}
