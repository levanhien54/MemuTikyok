//! ADB Worker (§ thiết kế Backup/Restore §4). Trích xuất/nạp dữ liệu app TikTok
//! trong máy ảo qua `memuc adb` + root. Trừu tượng sau trait [`AdbWorker`]:
//! `RealAdbWorker` gọi memuc thật; `MockAdbWorker` mô phỏng thiết bị trong bộ nhớ
//! để test round-trip mà không cần MEmu.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::{AppError, AppResult};
use crate::model::{EmulatorTell, SnapshotMeta};
use crate::snapshot::sha256_file;

#[async_trait]
pub trait AdbWorker: Send + Sync {
    /// Backup thư mục data của `pkg` trong VM `idx` ra file `out` (archive).
    async fn backup(&self, idx: u32, pkg: &str, out: &Path) -> AppResult<SnapshotMeta>;
    /// Nạp `archive` vào `/data/data/<pkg>` của VM `idx` (kèm chown + restorecon).
    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()>;
    /// Đặt Android ID (qua adb, không phải khoá memuc — §15 thiết kế).
    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()>;
    /// Flash sạch dữ liệu app (force-stop + pm clear) — cho luồng swap tài khoản.
    async fn wipe_app(&self, idx: u32, pkg: &str) -> AppResult<()>;
    /// Chờ Android boot xong (`sys.boot_completed == 1`) thay vì sleep cố định.
    async fn wait_boot_completed(&self, idx: u32) -> AppResult<()>;
    /// Mở app Android (launcher intent).
    async fn start_app(&self, idx: u32, pkg: &str) -> AppResult<()>;
    /// Cài APK (vd TikTok) vào VM.
    async fn install_apk(&self, idx: u32, apk_path: &str) -> AppResult<()>;
    /// Gỡ/vô hiệu hóa một app khỏi user 0 (dùng để gỡ bloat).
    async fn disable_app(&self, idx: u32, pkg: &str) -> AppResult<()>;
    /// Liệt kê package bên thứ 3 (để chọn gỡ app thừa).
    async fn list_third_party_apps(&self, idx: u32) -> AppResult<Vec<String>>;
    /// Scan dấu vết emulator (native check qua adb) → báo cáo từng mục.
    async fn scan_emulator_tells(&self, idx: u32) -> AppResult<Vec<EmulatorTell>>;
    /// Ẩn/sửa các dấu vết SỬA ĐƯỢC (best-effort; ro.* cần reboot mới ăn).
    async fn harden(&self, idx: u32) -> AppResult<()>;
}

// ------------------------ Real (memuc adb) ------------------------

pub struct RealAdbWorker {
    memuc_path: PathBuf,
}

impl RealAdbWorker {
    pub fn new(memuc_path: impl Into<PathBuf>) -> Self {
        Self {
            memuc_path: memuc_path.into(),
        }
    }

    /// Chạy `memuc -i <idx> adb "<adb_arg>"`, trả về stdout dạng bytes.
    async fn adb(&self, idx: u32, adb_arg: &str) -> AppResult<Vec<u8>> {
        let output = Command::new(&self.memuc_path)
            .args(["-i", &idx.to_string(), "adb", adb_arg])
            .output()
            .await?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::CommandFailed(format!("adb '{adb_arg}': {err}")));
        }
        Ok(output.stdout)
    }

    /// getprop sạch nhiễu "already connected" của memuc adb.
    async fn prop(&self, idx: u32, name: &str) -> String {
        let out = self
            .adb(idx, &format!("shell getprop {name}"))
            .await
            .unwrap_or_default();
        String::from_utf8_lossy(&out)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.contains("already connected"))
            .collect::<Vec<_>>()
            .join("")
    }
}

#[async_trait]
impl AdbWorker for RealAdbWorker {
    async fn backup(&self, idx: u32, pkg: &str, out: &Path) -> AppResult<SnapshotMeta> {
        // Dừng app sạch để flush SQLite ra đĩa (§14.1).
        self.adb(idx, &format!("shell su -c 'am force-stop {pkg}'"))
            .await?;
        // tar chỉ thư mục cần, loại cache; lấy binary qua exec-out.
        let tar_cmd = format!(
            "exec-out su -c 'cd /data/data/{pkg} && tar \
             --exclude=cache --exclude=code_cache --exclude=app_webview/*/GPUCache \
             -cf - shared_prefs databases files app_webview'"
        );
        let tar = self.adb(idx, &tar_cmd).await?;
        fs::write(out, &tar)?;

        let apk = String::from_utf8_lossy(
            &self
                .adb(
                    idx,
                    &format!("shell dumpsys package {pkg} | grep versionName"),
                )
                .await
                .unwrap_or_default(),
        )
        .trim()
        .to_string();

        Ok(SnapshotMeta {
            sha256: sha256_file(out)?,
            size_bytes: tar.len() as u64,
            apk_version: if apk.is_empty() {
                "unknown".into()
            } else {
                apk
            },
        })
    }

    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()> {
        let name = archive
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("snapshot.tar");
        let remote = format!("/data/local/tmp/{name}");

        self.adb(idx, &format!("shell su -c 'am force-stop {pkg}'"))
            .await?;
        // Bọc nháy đường dẫn cục bộ: thư mục temp/tên user có thể chứa khoảng trắng.
        self.adb(idx, &format!("push \"{}\" {remote}", archive.display()))
            .await?;
        // Giải nén vào /data/data (tar; nếu là .zst cần giải nén trước — xem runbook §14.2).
        self.adb(
            idx,
            &format!("shell su -c 'tar -xf {remote} -C /data/data/{pkg}'"),
        )
        .await?;
        // BẮT BUỘC: sửa chủ sở hữu theo UID hiện tại + nhãn SELinux (§14.2 / R-14).
        let fix = format!(
            "shell su -c 'U=$(stat -c %U /data/data/{pkg}); \
             chown -R $U:$U /data/data/{pkg} && restorecon -R /data/data/{pkg}'"
        );
        self.adb(idx, &fix).await?;
        self.adb(idx, &format!("shell su -c 'rm -f {remote}'"))
            .await?;
        Ok(())
    }

    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()> {
        self.adb(
            idx,
            &format!("shell su -c 'settings put secure android_id {android_id}'"),
        )
        .await
        .map(|_| ())
    }

    async fn wipe_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        // pm clear / force-stop chạy được với shell user, KHÔNG cần root.
        self.adb(idx, &format!("shell am force-stop {pkg}")).await?;
        self.adb(idx, &format!("shell pm clear {pkg}")).await?;
        Ok(())
    }

    async fn wait_boot_completed(&self, idx: u32) -> AppResult<()> {
        use tokio::time::{sleep, Duration};
        // Poll tối đa ~90s. `memuc adb` có thể chèn dòng "already connected ..."
        // trước giá trị → lấy token cuối cùng để so sánh (bug đã gặp trên MEmu thật).
        for _ in 0..30 {
            let out = self
                .adb(idx, "shell getprop sys.boot_completed")
                .await
                .unwrap_or_default();
            let text = String::from_utf8_lossy(&out);
            if text.split_whitespace().last() == Some("1") {
                return Ok(());
            }
            sleep(Duration::from_secs(3)).await;
        }
        Err(AppError::Timeout(90))
    }

    async fn start_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        self.adb(
            idx,
            &format!("shell monkey -p {pkg} -c android.intent.category.LAUNCHER 1"),
        )
        .await
        .map(|_| ())
    }

    async fn install_apk(&self, idx: u32, apk_path: &str) -> AppResult<()> {
        // -r: cài đè nếu đã có; -g: cấp sẵn quyền runtime.
        self.adb(idx, &format!("install -r -g \"{apk_path}\""))
            .await
            .map(|_| ())
    }

    async fn disable_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        // Shell user có quyền disable-user (kiểm chứng trên MEmu thật) — không cần root.
        self.adb(idx, &format!("shell pm disable-user --user 0 {pkg}"))
            .await
            .map(|_| ())
    }

    async fn list_third_party_apps(&self, idx: u32) -> AppResult<Vec<String>> {
        let out = self.adb(idx, "shell pm list packages -3").await?;
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|l| l.trim().strip_prefix("package:").map(str::to_string))
            .collect())
    }

    async fn scan_emulator_tells(&self, idx: u32) -> AppResult<Vec<EmulatorTell>> {
        let mut tells = Vec::new();

        let nb = self.prop(idx, "ro.dalvik.vm.native.bridge").await;
        tells.push(EmulatorTell {
            check: "Native Bridge (ARM→x86)".into(),
            detected: !nb.is_empty() && nb != "0",
            detail: nb,
        });

        let cpu = self
            .adb(idx, "shell cat /proc/cpuinfo")
            .await
            .unwrap_or_default();
        let hyp = String::from_utf8_lossy(&cpu).contains("hypervisor");
        tells.push(EmulatorTell {
            check: "CPU hypervisor flag".into(),
            detected: hyp,
            detail: if hyp {
                "cpuinfo có 'hypervisor'"
            } else {
                "sạch"
            }
            .into(),
        });

        let qk = self.prop(idx, "ro.kernel.qemu").await;
        tells.push(EmulatorTell {
            check: "ro.kernel.qemu".into(),
            detected: qk == "1",
            detail: if qk.is_empty() { "rỗng".into() } else { qk },
        });

        let mut found = Vec::new();
        for f in [
            "/dev/qemu_pipe",
            "/dev/socket/qemud",
            "/dev/socket/genyd",
            "/system/lib/libc_malloc_debug_qemu.so",
        ] {
            let r = self
                .adb(idx, &format!("shell ls {f}"))
                .await
                .unwrap_or_default();
            let s = String::from_utf8_lossy(&r);
            if s.contains(f) && !s.contains("No such") {
                found.push(f);
            }
        }
        tells.push(EmulatorTell {
            check: "File QEMU/Genymotion".into(),
            detected: !found.is_empty(),
            detail: if found.is_empty() {
                "sạch".into()
            } else {
                found.join(", ")
            },
        });

        let mounts = self
            .adb(idx, "shell cat /proc/mounts")
            .await
            .unwrap_or_default();
        let vbox = String::from_utf8_lossy(&mounts).contains("vboxsf");
        tells.push(EmulatorTell {
            check: "vboxsf mount".into(),
            detected: vbox,
            detail: if vbox { "có" } else { "sạch" }.into(),
        });

        let sf = self
            .adb(idx, "shell dumpsys SurfaceFlinger")
            .await
            .unwrap_or_default();
        let sfs = String::from_utf8_lossy(&sf);
        let bad_gpu =
            sfs.contains("SwiftShader") || sfs.contains("llvmpipe") || sfs.contains("VirGL");
        tells.push(EmulatorTell {
            check: "GPU renderer ảo".into(),
            detected: bad_gpu,
            detail: if bad_gpu {
                "SwiftShader/llvmpipe"
            } else {
                "GPU thật (Adreno/Mali)"
            }
            .into(),
        });

        let bc = self.prop(idx, "ro.build.characteristics").await;
        tells.push(EmulatorTell {
            check: "ro.build.characteristics".into(),
            detected: bc.contains("tablet"),
            detail: bc,
        });

        Ok(tells)
    }

    async fn harden(&self, idx: u32) -> AppResult<()> {
        // Xóa prop camera giả (runtime-settable).
        let _ = self.adb(idx, "shell setprop qemu.sf.fake_camera ''").await;
        // Sửa ro.build.characteristics qua build.prop (cần root + remount; ăn sau reboot).
        let _ = self
            .adb(
                idx,
                "shell su -c \"mount -o rw,remount /system; sed -i 's/ro.build.characteristics=tablet/ro.build.characteristics=default/' /system/build.prop\"",
            )
            .await;
        Ok(())
    }
}

// ------------------------ Mock (in-memory) ------------------------

/// Mô phỏng thiết bị: giữ "dữ liệu app" theo index trong bộ nhớ. Cho phép test
/// vòng tròn backup→restore mà không cần MEmu/root.
pub struct MockAdbWorker {
    devices: Mutex<HashMap<u32, Vec<u8>>>,
    android_ids: Mutex<HashMap<u32, String>>,
}

impl MockAdbWorker {
    pub fn new() -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            android_ids: Mutex::new(HashMap::new()),
        }
    }

    /// Set dữ liệu app cho một VM (dùng trong test/dev).
    #[cfg(test)]
    pub fn set_device_data(&self, idx: u32, data: Vec<u8>) {
        self.devices.lock().unwrap().insert(idx, data);
    }

    #[cfg(test)]
    pub fn device_data(&self, idx: u32) -> Option<Vec<u8>> {
        self.devices.lock().unwrap().get(&idx).cloned()
    }

    #[cfg(test)]
    pub fn clear_devices(&self) {
        self.devices.lock().unwrap().clear();
    }

    #[cfg(test)]
    pub fn android_id_of(&self, idx: u32) -> Option<String> {
        self.android_ids.lock().unwrap().get(&idx).cloned()
    }
}

impl Default for MockAdbWorker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AdbWorker for MockAdbWorker {
    async fn backup(&self, idx: u32, _pkg: &str, out: &Path) -> AppResult<SnapshotMeta> {
        let data = self
            .devices
            .lock()
            .unwrap()
            .get(&idx)
            .cloned()
            .unwrap_or_else(|| format!("mock-tiktok-data-{idx}").into_bytes());
        fs::write(out, &data)?;
        Ok(SnapshotMeta {
            sha256: sha256_file(out)?,
            size_bytes: data.len() as u64,
            apk_version: "mock-1.0".into(),
        })
    }

    async fn restore(&self, idx: u32, _pkg: &str, archive: &Path) -> AppResult<()> {
        let data = fs::read(archive)?;
        self.devices.lock().unwrap().insert(idx, data);
        Ok(())
    }

    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()> {
        self.android_ids
            .lock()
            .unwrap()
            .insert(idx, android_id.to_string());
        Ok(())
    }

    async fn wipe_app(&self, idx: u32, _pkg: &str) -> AppResult<()> {
        self.devices.lock().unwrap().remove(&idx);
        Ok(())
    }

    async fn wait_boot_completed(&self, _idx: u32) -> AppResult<()> {
        Ok(())
    }

    async fn start_app(&self, _idx: u32, _pkg: &str) -> AppResult<()> {
        Ok(())
    }

    async fn install_apk(&self, _idx: u32, _apk_path: &str) -> AppResult<()> {
        Ok(())
    }

    async fn disable_app(&self, _idx: u32, _pkg: &str) -> AppResult<()> {
        Ok(())
    }

    async fn list_third_party_apps(&self, _idx: u32) -> AppResult<Vec<String>> {
        Ok(vec![])
    }

    async fn scan_emulator_tells(&self, _idx: u32) -> AppResult<Vec<EmulatorTell>> {
        // Mẫu khớp thực tế MEmu: chỉ còn native-bridge + hypervisor lộ.
        Ok(vec![
            EmulatorTell {
                check: "Native Bridge (ARM→x86)".into(),
                detected: true,
                detail: "libnb.so".into(),
            },
            EmulatorTell {
                check: "CPU hypervisor flag".into(),
                detected: true,
                detail: "cpuinfo có 'hypervisor'".into(),
            },
            EmulatorTell {
                check: "ro.kernel.qemu".into(),
                detected: false,
                detail: "rỗng".into(),
            },
            EmulatorTell {
                check: "File QEMU/Genymotion".into(),
                detected: false,
                detail: "sạch".into(),
            },
            EmulatorTell {
                check: "GPU renderer ảo".into(),
                detected: false,
                detail: "GPU thật (Adreno/Mali)".into(),
            },
        ])
    }

    async fn harden(&self, _idx: u32) -> AppResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mpm_adb_{}_{tag}.bin", std::process::id()))
    }

    #[tokio::test]
    async fn backup_roi_restore_giu_nguyen_du_lieu() {
        let w = MockAdbWorker::new();
        w.set_device_data(1, b"session-cookies-account-1".to_vec());

        let archive = tmp("arc");
        let meta = w
            .backup(1, "com.zhiliaoapp.musically", &archive)
            .await
            .unwrap();
        assert_eq!(meta.size_bytes, "session-cookies-account-1".len() as u64);
        assert_eq!(meta.sha256.len(), 64);

        // Nạp sang VM index khác → dữ liệu phải khớp bản backup.
        w.restore(2, "com.zhiliaoapp.musically", &archive)
            .await
            .unwrap();
        let restored = w.devices.lock().unwrap().get(&2).cloned().unwrap();
        assert_eq!(restored, b"session-cookies-account-1");

        let _ = fs::remove_file(&archive);
    }
}
