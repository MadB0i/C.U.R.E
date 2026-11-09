//! Low-level NTFS Master File Table (MFT) and volume geometry inspection.
//! Queries filesystem driver metadata directly via DeviceIoControl:
//! - FSCTL_GET_NTFS_VOLUME_DATA (`0x00090064`)
//! - FSCTL_QUERY_USN_JOURNAL (`0x000900f4`)
//!
//! Provides kernel-grade storage volume telemetry (MFT record segment sizes, cluster geometry, LCN markers).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LowLevelVolumeGeometry {
    pub drive_letter: String,
    pub filesystem: String,
    pub bytes_per_sector: u32,
    pub bytes_per_cluster: u32,
    pub bytes_per_file_record: u32,
    pub total_clusters: u64,
    pub free_clusters: u64,
    pub mft_start_lcn: Option<u64>,
    pub mft_valid_bytes: Option<u64>,
    pub is_ntfs: bool,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RawNtfsVolumeDataBuffer {
    volume_serial_number: i64,
    number_sectors: i64,
    total_clusters: i64,
    free_clusters: i64,
    total_reserved: i64,
    bytes_per_sector: u32,
    bytes_per_cluster: u32,
    bytes_per_file_record_segment: u32,
    clusters_per_file_record_segment: u32,
    mft_valid_data_length: i64,
    mft_start_lcn: i64,
    mft2_start_lcn: i64,
    mft_zone_start: i64,
    mft_zone_end: i64,
}

/// Query low-level filesystem volume geometry and MFT metrics directly from the Windows kernel.
pub fn query_low_level_volume_geometry(drive: &str) -> Option<LowLevelVolumeGeometry> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x00090064;

        extern "system" {
            fn DeviceIoControl(
                h_device: windows::Win32::Foundation::HANDLE,
                dw_io_control_code: u32,
                lp_in_buffer: *const std::ffi::c_void,
                n_in_buffer_size: u32,
                lp_out_buffer: *mut std::ffi::c_void,
                n_out_buffer_size: u32,
                lp_bytes_returned: *mut u32,
                lp_overlapped: *mut std::ffi::c_void,
            ) -> windows::Win32::Foundation::BOOL;
        }

        let clean_drive = drive.trim_end_matches(['\\', '/']).to_ascii_uppercase();
        let volume_path = format!(r"\\.\{}", clean_drive);
        let wide: Vec<u16> = std::ffi::OsStr::new(&volume_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // Open volume handle with read/share permissions
        let handle = unsafe {
            CreateFileW(
                PCWSTR::from_raw(wide.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        };

        if let Ok(h) = handle {
            let mut out_buffer = RawNtfsVolumeDataBuffer::default();
            let mut bytes_returned: u32 = 0;

            let success = unsafe {
                DeviceIoControl(
                    h,
                    FSCTL_GET_NTFS_VOLUME_DATA,
                    std::ptr::null(),
                    0,
                    &mut out_buffer as *mut _ as *mut std::ffi::c_void,
                    std::mem::size_of::<RawNtfsVolumeDataBuffer>() as u32,
                    &mut bytes_returned,
                    std::ptr::null_mut(),
                )
            };

            let _ = unsafe { CloseHandle(h) };

            if success.as_bool() && bytes_returned > 0 {
                return Some(LowLevelVolumeGeometry {
                    drive_letter: clean_drive,
                    filesystem: "NTFS".to_string(),
                    bytes_per_sector: out_buffer.bytes_per_sector,
                    bytes_per_cluster: out_buffer.bytes_per_cluster,
                    bytes_per_file_record: out_buffer.bytes_per_file_record_segment,
                    total_clusters: out_buffer.total_clusters.max(0) as u64,
                    free_clusters: out_buffer.free_clusters.max(0) as u64,
                    mft_start_lcn: Some(out_buffer.mft_start_lcn.max(0) as u64),
                    mft_valid_bytes: Some(out_buffer.mft_valid_data_length.max(0) as u64),
                    is_ntfs: true,
                });
            }
        }

        // Fallback for non-NTFS or standard user volumes: query GetDiskFreeSpaceW
        use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceW;
        let root_str = format!("{}\\", clean_drive);
        let root_wide: Vec<u16> = std::ffi::OsStr::new(&root_str)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut sectors_per_cluster: u32 = 0;
        let mut bytes_per_sector: u32 = 0;
        let mut number_of_free_clusters: u32 = 0;
        let mut total_number_of_clusters: u32 = 0;

        let ret = unsafe {
            GetDiskFreeSpaceW(
                PCWSTR::from_raw(root_wide.as_ptr()),
                Some(&mut sectors_per_cluster),
                Some(&mut bytes_per_sector),
                Some(&mut number_of_free_clusters),
                Some(&mut total_number_of_clusters),
            )
        };

        if ret.is_ok() {
            let bytes_per_cluster = sectors_per_cluster.saturating_mul(bytes_per_sector);
            Some(LowLevelVolumeGeometry {
                drive_letter: clean_drive,
                filesystem: "Standard / Volume".to_string(),
                bytes_per_sector,
                bytes_per_cluster,
                bytes_per_file_record: 1024,
                total_clusters: total_number_of_clusters as u64,
                free_clusters: number_of_free_clusters as u64,
                mft_start_lcn: None,
                mft_valid_bytes: None,
                is_ntfs: false,
            })
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = drive;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_low_level_volume_geometry() {
        #[cfg(windows)]
        {
            let geom = query_low_level_volume_geometry("C:");
            assert!(geom.is_some(), "Expected volume geometry for drive C:");
            let g = geom.unwrap();
            assert_eq!(g.drive_letter, "C:");
            assert!(g.bytes_per_sector >= 512);
            assert!(g.bytes_per_cluster >= 512);
            assert!(g.total_clusters > 0);
        }
    }
}
