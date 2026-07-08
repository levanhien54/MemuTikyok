//! ADB Worker (§ thiết kế Backup/Restore §4). Trích xuất/nạp dữ liệu app TikTok
//! trong máy ảo qua `MuMuManager.exe adb` + root. Trừu tượng sau trait [`AdbWorker`]:
//! `RealAdbWorker` gọi MuMuManager thật; `MockAdbWorker` mô phỏng thiết bị trong bộ nhớ
//! để test round-trip mà không cần MuMu.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::time::{sleep, timeout, Duration};

use crate::error::{AppError, AppResult};
use crate::humanize::{self, Rng};
use crate::model::{EmulatorTell, HardwareProfile, SnapshotMeta};
use crate::snapshot::sha256_file;

/// Trần thời gian cho một lệnh `MuMuManager.exe adb`. Đủ rộng cho thao tác nặng nhất
/// (install APK ~220MB, backup/restore) nhưng vẫn chặn treo vô hạn nếu adb đơ.
const ADB_TIMEOUT: Duration = Duration::from_secs(300);

#[async_trait]
pub trait AdbWorker: Send + Sync {
    /// Backup thư mục data của `pkg` trong VM `idx` ra file `out` (archive).
    async fn backup(&self, idx: u32, pkg: &str, out: &Path) -> AppResult<SnapshotMeta>;
    /// Nạp `archive` vào `/data/data/<pkg>` của VM `idx` (kèm chown + restorecon).
    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()>;
    /// Phiên bản APK hiện đang cài trong VM. Dùng để chặn restore snapshot lệch schema.
    async fn apk_version(&self, idx: u32, pkg: &str) -> AppResult<String>;
    /// Đặt Android ID (qua adb, không phải khoá MuMuManager — §15 thiết kế).
    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()>;
    /// Force logical display size/density after Android boot. MuMu `custom_resolution`
    /// can configure the VM window while Android still reports its default `wm` metrics.
    async fn apply_display_profile(
        &self,
        idx: u32,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> AppResult<bool>;
    /// Chờ Android boot xong (`sys.boot_completed == 1`) thay vì sleep cố định.
    async fn wait_boot_completed(&self, idx: u32) -> AppResult<()>;
    /// Mở app Android (launcher intent).
    async fn start_app(&self, idx: u32, pkg: &str) -> AppResult<()>;
    /// Cài APK (vd TikTok) vào VM.
    async fn install_apk(&self, idx: u32, apk_path: &str) -> AppResult<()>;
    /// Gỡ/vô hiệu hóa một app khỏi user 0 (dùng để gỡ bloat).
    async fn disable_app(&self, idx: u32, pkg: &str) -> AppResult<()>;
    /// Scan dấu vết emulator (native check qua adb) → báo cáo từng mục.
    async fn scan_emulator_tells(&self, idx: u32) -> AppResult<Vec<EmulatorTell>>;
    /// Ẩn/sửa các dấu vết runtime best-effort; ro.* được xử lý bằng resetprop trong lock_device_identity.
    async fn harden(&self, idx: u32) -> AppResult<()>;
    /// Đẩy binary `magisk` (chứa applet resetprop, trích từ Magisk APK) vào VM tại
    /// `/data/local/tmp/magisk` + chmod + verify (`magisk -c`). VM đã có root (enable_su)
    /// nên KHÔNG cần cài Magisk hệ thống. Trả `Ok(true)` nếu binary chạy được.
    async fn push_resetprop(&self, idx: u32, local_bin: &str) -> AppResult<bool>;
    /// KHÓA định danh thiết bị (ro.product.model/brand/manufacturer/device +
    /// ro.build.fingerprint) SAU boot bằng **resetprop** — chống MuMu random model.
    /// Best-effort: trả `Ok(false)` nếu VM chưa có resetprop/magisk (đặt Magisk APK trong
    /// Cài đặt — xem docs/BASE_IMAGE_MAGISK_SETUP.md); `Ok(true)` nếu khóa & verify được.
    async fn lock_device_identity(&self, idx: u32, hw: &HardwareProfile) -> AppResult<bool>;
    /// Tap có rung tọa độ + thời gian giữ ngẫu nhiên. ⚠️ HẠN CHẾ THẬT: hiện dùng
    /// `input swipe pt pt hold` — sự kiện BƠM (injected, không có luồng touch
    /// DOWN/MOVE/UP thật, không áp lực/kích thước) → VẪN có thể bị phát hiện. Không
    /// coi đây là "chống dò tự động hóa" hoàn chỉnh (cần sendevent /dev/input — TODO).
    async fn human_tap(&self, idx: u32, x: i32, y: i32) -> AppResult<()>;
    /// Swipe có rung + thời lượng ngẫu nhiên. ⚠️ HẠN CHẾ THẬT: đường cong Bézier +
    /// gia tốc do humanize.rs tính bị BỎ, chỉ 2 điểm đầu/cuối đưa vào `input swipe`
    /// → Android nội suy ĐƯỜNG THẲNG vận tốc tuyến tính (một dấu hiệu bot rõ). Chưa
    /// đạt "chống touch-jitter check"; cần sendevent phát lại toàn đường (TODO).
    async fn human_swipe(&self, idx: u32, x0: i32, y0: i32, x1: i32, y1: i32) -> AppResult<()>;
    /// Nạp file media (video, ảnh) từ máy tính vào máy ảo (Mục Camera)
    /// và gọi broadcast quét media để xuất hiện ngay trong thư viện.
    async fn upload_media(&self, idx: u32, local_path: &str) -> AppResult<()>;
}

fn sh_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn build_lock_script(rp: &str, hw: &HardwareProfile) -> String {
    let mut script = String::from("#!/system/bin/sh\n");

    for p in ["ro.kernel.qemu", "ro.boot.qemu", "ro.mumu.version"] {
        script.push_str(&format!("{rp} --delete {p}\n"));
    }

    let core: [(&str, &str); 7] = [
        ("ro.product.model", &hw.model),
        ("ro.product.brand", &hw.brand),
        ("ro.product.manufacturer", &hw.manufacturer),
        ("ro.product.device", &hw.device),
        ("ro.product.name", &hw.device),
        ("ro.build.fingerprint", &hw.build_fingerprint),
        ("ro.product.board", &hw.device),
    ];
    for (key, val) in core {
        if !val.is_empty() {
            script.push_str(&format!("{rp} {key} '{}'\n", sh_escape(val)));
        }
    }

    let coherent: [(&str, &str); 5] = [
        ("ro.hardware", &hw.soc_hardware),
        ("ro.board.platform", &hw.board_platform),
        ("ro.hardware.egl", &hw.gpu_egl),
        ("ro.build.version.security_patch", &hw.security_patch),
        ("ro.build.characteristics", &hw.build_characteristics),
    ];
    for (key, val) in coherent {
        if val.is_empty() {
            if key == "ro.build.characteristics" {
                script.push_str(&format!("{rp} --delete {key}\n"));
            }
            continue;
        }
        script.push_str(&format!("{rp} {key} '{}'\n", sh_escape(val)));
    }

    for (key, val) in [
        ("ro.build.tags", "release-keys"),
        ("ro.build.type", "user"),
        ("ro.secure", "1"),
        ("ro.debuggable", "0"),
    ] {
        script.push_str(&format!("{rp} {key} '{val}'\n"));
    }
    script.push_str(&format!("{rp} sys.usb.state 'mtp'\n"));

    for f in [
        "/dev/qemu_pipe",
        "/dev/socket/qemud",
        "/dev/socket/genyd",
        "/system/lib/vboxguest.ko",
        "/system/bin/nemuVM-tools",
        "/system/xbin/nemuVM-tools",
    ] {
        script.push_str(&format!(
            "if [ -e {f} ]; then mount -o bind /dev/null {f}; fi\n"
        ));
    }

    script
}

// ------------------------ Real (MuMuManager adb) ------------------------

pub struct RealAdbWorker {
    manager_path: PathBuf,
}

impl RealAdbWorker {
    pub fn new(manager_path: impl Into<PathBuf>) -> Self {
        Self {
            manager_path: manager_path.into(),
        }
    }

    /// Chạy `MuMuManager.exe adb -v <idx> -c "<adb_arg>"`, trả về stdout dạng bytes.
    ///
    /// - **CREATE_NO_WINDOW**: ẩn cửa sổ console → KHÔNG nhấp nháy khi poll boot
    ///   (mỗi 3s) hay gọi adb liên tục (fix "cửa sổ cmd chớp nháy").
    /// - **kill_on_drop + timeout**: hết giờ thì hủy tiến trình con, không treo vô hạn.
    async fn adb(&self, idx: u32, adb_arg: &str) -> AppResult<Vec<u8>> {
        let mut cmd = Command::new(&self.manager_path);
        cmd.args(["adb", "-v", &idx.to_string(), "-c", adb_arg]);
        cmd.kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let output = timeout(ADB_TIMEOUT, cmd.output())
            .await
            .map_err(|_| AppError::Timeout(ADB_TIMEOUT.as_secs()))?
            .map_err(command_error)?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::CommandFailed(format!("adb '{adb_arg}': {err}")));
        }
        Ok(output.stdout)
    }

    /// Chạy `MuMuManager.exe adb -v <idx> -c "<args...>"` với mảng tham số trực tiếp.
    /// Giúp tránh lỗi shlex phân tích đường dẫn Windows (mất dấu \).
    async fn adb_args(&self, idx: u32, adb_args: &[&str]) -> AppResult<Vec<u8>> {
        let mut joined = String::new();
        for arg in adb_args {
            if !joined.is_empty() {
                joined.push(' ');
            }
            if arg.contains(' ') {
                joined.push_str(&format!("\"{}\"", arg));
            } else {
                joined.push_str(arg);
            }
        }
        let mut cmd = Command::new(&self.manager_path);
        cmd.args(["adb", "-v", &idx.to_string(), "-c", &joined]);
        cmd.kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let output = timeout(ADB_TIMEOUT, cmd.output())
            .await
            .map_err(|_| AppError::Timeout(ADB_TIMEOUT.as_secs()))?
            .map_err(command_error)?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::CommandFailed(format!(
                "adb {:?}: {}",
                adb_args, err
            )));
        }
        Ok(output.stdout)
    }

    /// getprop sạch nhiễu MuMuManager adb. `getprop <name>` cho DÚNG MỘT dòng giá trị; mọi nhiễu
    /// ("already connected", "daemon started"...) MuMuManager chèn nằm TRƯỚC → lấy dòng SẠCH CUỐI
    /// cùng. KHÔNG join tất cả (nhiễu sẽ dính vào value → verify sai — finding resolve-logic).
    async fn prop(&self, idx: u32, name: &str) -> String {
        let out = self
            .adb(idx, &format!("shell getprop {name}"))
            .await
            .unwrap_or_default();
        String::from_utf8_lossy(&out)
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.contains("already connected"))
            .unwrap_or("")
            .to_string()
    }
}

fn command_error(e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::NotFound {
        AppError::EmulatorNotFound
    } else {
        AppError::Io(e.to_string())
    }
}

fn tar_archive_looks_valid(path: &Path) -> AppResult<bool> {
    let bytes = fs::read(path)?;
    if bytes.len() < 1024 || bytes.len() % 512 != 0 {
        return Ok(false);
    }

    let mut offset = 0usize;
    let mut entries = 0usize;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|&b| b == 0) {
            return Ok(entries > 0);
        }

        let name = &header[..100];
        if name.iter().all(|&b| b == 0) {
            return Ok(false);
        }

        let size_field = &header[124..136];
        let size_text: String = size_field
            .iter()
            .copied()
            .take_while(|&b| b != 0 && b != b' ')
            .map(char::from)
            .collect();
        let size = if size_text.trim().is_empty() {
            0usize
        } else {
            usize::from_str_radix(size_text.trim(), 8).unwrap_or(0)
        };
        entries += 1;
        let data_blocks = size.div_ceil(512);
        offset += 512 + data_blocks * 512;
    }

    Ok(false)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn safe_remote_media_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len().max(1));
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches([' ', '.']).trim();
    if trimmed.is_empty() {
        "media.bin".to_string()
    } else {
        trimmed.to_string()
    }
}

#[async_trait]
impl AdbWorker for RealAdbWorker {
    async fn backup(&self, idx: u32, pkg: &str, out: &Path) -> AppResult<SnapshotMeta> {
        // Đảm bảo adb chạy dưới quyền root (MuMu 12 không có `su`, phải dùng `adb root`).
        let _ = self.adb(idx, "root").await;
        // Dừng app sạch để flush SQLite ra đĩa (§14.1).
        self.adb(idx, &format!("shell am force-stop {pkg}")).await?;
        // Tar TRÊN THIẾT BỊ ra FILE rồi `adb pull`
        let remote = format!("/data/local/tmp/mpm-backup-{idx}.tar");
        let tar_cmd = format!(
            "shell cd /data/data/{pkg} && tar \
             --exclude=cache --exclude=code_cache --exclude=app_webview/*/GPUCache \
             --exclude=files/aweme --exclude=files/music --exclude=files/offline --exclude=files/splash --exclude=files/logs \
             --exclude=files/cache --exclude=files/plugins --exclude=files/unzip --exclude=files/debug \
             -cf {remote} shared_prefs databases files app_webview; chmod 644 {remote}"
        );
        self.adb(idx, &tar_cmd).await?;
        // pull nhị phân-an-toàn (bọc nháy đường dẫn cục bộ: có thể chứa khoảng trắng).
        self.adb(idx, &format!("pull {remote} \"{}\"", out.display()))
            .await?;
        let _ = self.adb(idx, &format!("shell rm -f {remote}")).await;

        // Chặn snapshot RỖNG (tar thất bại toàn bộ / pkg dir không tồn tại) — R-15 không
        // được ghi bản backup ma rồi hủy VM. Archive hợp lệ tối thiểu (chỉ files/) ~2.5KB.
        let size = fs::metadata(out).map(|m| m.len()).unwrap_or(0);
        if size == 0 || !tar_archive_looks_valid(out)? {
            return Err(AppError::CommandFailed(format!(
                "backup: archive rỗng/không hợp lệ cho {pkg} (VM idx {idx}) — không có dữ liệu để lưu"
            )));
        }

        let apk = self
            .apk_version(idx, pkg)
            .await
            .unwrap_or_else(|_| "unknown".into());

        Ok(SnapshotMeta {
            sha256: sha256_file(out)?,
            size_bytes: size,
            apk_version: apk,
        })
    }

    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()> {
        let _ = self.adb(idx, "root").await;
        let name = archive
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("snapshot.tar");
        let remote = format!("/data/local/tmp/{name}");

        self.adb(idx, &format!("shell am force-stop {pkg}")).await?;
        // Bọc nháy đường dẫn cục bộ: thư mục temp/tên user có thể chứa khoảng trắng.
        self.adb(idx, &format!("push \"{}\" {remote}", archive.display()))
            .await?;
        // Giải nén vào /data/data (tar; nếu là .zst cần giải nén trước — xem runbook §14.2).
        self.adb(idx, &format!("shell tar -xf {remote} -C /data/data/{pkg}"))
            .await?;
        // BẮT BUỘC: sửa chủ sở hữu theo UID hiện tại + nhãn SELinux (§14.2 / R-14).
        let fix = format!(
            "shell U=$(stat -c %u /data/data/{pkg}); \
             chown -R $U:$U /data/data/{pkg} && restorecon -R /data/data/{pkg}"
        );
        self.adb(idx, &fix).await?;
        self.adb(idx, &format!("shell rm -f {remote}")).await?;
        Ok(())
    }

    async fn apk_version(&self, idx: u32, pkg: &str) -> AppResult<String> {
        let out = self
            .adb(
                idx,
                &format!("shell dumpsys package {pkg} | grep versionName"),
            )
            .await?;
        let version = String::from_utf8_lossy(&out).trim().to_string();
        Ok(if version.is_empty() {
            "unknown".into()
        } else {
            version
        })
    }

    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()> {
        let _ = self.adb(idx, "root").await;
        self.adb(
            idx,
            &format!("shell settings put secure android_id {android_id}"),
        )
        .await
        .map(|_| ())
    }

    async fn apply_display_profile(
        &self,
        idx: u32,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> AppResult<bool> {
        if width == 0 || height == 0 || dpi < 72 {
            return Ok(false);
        }

        let _ = self.adb(idx, "root").await;
        self.adb(idx, &format!("shell wm size {width}x{height}"))
            .await?;
        self.adb(idx, &format!("shell wm density {dpi}")).await?;
        sleep(Duration::from_millis(500)).await;

        let size_out = self.adb(idx, "shell wm size").await.unwrap_or_default();
        let density_out = self.adb(idx, "shell wm density").await.unwrap_or_default();
        let size_text = String::from_utf8_lossy(&size_out);
        let density_text = String::from_utf8_lossy(&density_out);
        let expected_size = format!("{width}x{height}");
        let expected_density = dpi.to_string();
        let size_ok = size_text.contains(&expected_size);
        let density_ok = density_text
            .split(|c: char| !c.is_ascii_digit())
            .any(|token| token == expected_density);

        if !size_ok || !density_ok {
            tracing::warn!(
                idx,
                expected_size,
                expected_density,
                size = %size_text.trim(),
                density = %density_text.trim(),
                "wm size/density khong khop profile sau khi apply"
            );
        }

        Ok(size_ok && density_ok)
    }

    async fn wait_boot_completed(&self, idx: u32) -> AppResult<()> {
        use tokio::time::sleep;
        // Poll tối đa ~180s. Boot có thể vượt 90s khi host tải nặng (nhiều VM chạy) —
        // phát hiện qua test thực. `MuMuManager adb` có thể chèn dòng "already connected ..."
        // trước giá trị → lấy token cuối cùng để so sánh (bug đã gặp trên MuMu thật).
        for _ in 0..60 {
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
        Err(AppError::Timeout(180))
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
        // ⚠️ --no-streaming BẮT BUỘC: MuMu KHÔNG hỗ trợ streamed install — adb mặc định
        //    dùng streamed → "adb: failed to install ...: Performing Streamed Install"
        //    (thất bại tức thì, không push byte nào; kiểm chứng qua test thực). Ép chế độ
        //    push-rồi-install (--no-streaming) mới cài được (230MB push ~12s → "Success").
        // ⚠️ `adb install` báo kết quả ở OUTPUT chứ không chỉ ở exit code: nhiều bản vẫn
        //    exit 0 dù thất bại → PHẢI đọc output tìm "Success", không tin mỗi status.
        //    Bắt cả stderr để báo lỗi rõ ràng (không đi qua adb() vì nó bỏ stderr khi exit 0).
        // ⚠️ THỬ LẠI: ngay sau khi provision boot + áp android_id/debloat/harden, kết nối
        //    adb hay bị "failed to read copy response" (rớt lúc commit dù push xong 230MB).
        //    Đây là lỗi CHỚP NHOÁNG — thử lại vài lần (chờ VM lắng) là ăn (kiểm chứng thực).
        // Chống inject vào chuỗi shell: apk_path được bọc trong nháy kép, nên một ký tự
        // nháy/metachar có thể thoát nháy và chèn token adb/shell khác. Từ chối sớm.
        if apk_path.contains(['"', '\'', '`', '$', ';', '&', '|', '\n', '\r']) {
            return Err(AppError::InvalidInput(format!(
                "Đường dẫn APK chứa ký tự không hợp lệ: {apk_path}"
            )));
        }
        let arg = format!("install -r -g --no-streaming \"{apk_path}\"");
        const MAX_TRIES: u32 = 3;
        let mut last = String::new();
        for attempt in 1..=MAX_TRIES {
            let mut cmd = Command::new(&self.manager_path);
            cmd.args(["adb", "-v", &idx.to_string(), "-c", &arg]);
            cmd.kill_on_drop(true);
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let output = timeout(ADB_TIMEOUT, cmd.output())
                .await
                .map_err(|_| AppError::Timeout(ADB_TIMEOUT.as_secs()))?
                .map_err(command_error)?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stdout.contains("Success") || stderr.contains("Success") {
                return Ok(());
            }
            last = format!("out={}, err={}", stdout.trim(), stderr.trim());
            tracing::warn!(
                idx,
                attempt,
                max = MAX_TRIES,
                "adb install lỗi, thử lại: {last}"
            );
            if attempt < MAX_TRIES {
                sleep(Duration::from_secs(4)).await;
            }
        }
        Err(AppError::CommandFailed(format!(
            "adb install thất bại sau {MAX_TRIES} lần: {last}"
        )))
    }

    async fn disable_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        // Shell user có quyền disable-user (kiểm chứng trên MuMu thật) — không cần root.
        self.adb(idx, &format!("shell pm disable-user --user 0 {pkg}"))
            .await
            .map(|_| ())
    }

    async fn scan_emulator_tells(&self, idx: u32) -> AppResult<Vec<EmulatorTell>> {
        // MuMu 12 không có `su` — adbd chạy root qua `adb root`; các check dưới chạy trực tiếp.
        let _ = self.adb(idx, "root").await;
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
            "/system/bin/nemuVM-tools",
            "/system/xbin/nemuVM-tools",
        ] {
            // Nếu file bị mount đè (có trong /proc/mounts), tức là ta đã ẩn nó thành công
            let check_cmd = format!("if [ -e {f} ]; then if grep -q ' {f} ' /proc/mounts; then echo 'HIDDEN_MOUNT'; else echo 'EXISTS'; fi; else echo 'NOT_FOUND'; fi");
            let r = self
                .adb(idx, &format!("shell {check_cmd}"))
                .await
                .unwrap_or_default();
            let s = String::from_utf8_lossy(&r);
            if s.contains("EXISTS") {
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

        let features = self
            .adb(idx, "shell pm list features")
            .await
            .unwrap_or_default();
        let sensor_dump = self
            .adb(idx, "shell dumpsys sensorservice")
            .await
            .unwrap_or_default();
        let sensor_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&features),
            String::from_utf8_lossy(&sensor_dump)
        )
        .to_lowercase();
        let has_accel = sensor_text.contains("accelerometer");
        let has_gyro = sensor_text.contains("gyroscope") || sensor_text.contains(" gyro");
        let has_magnet = sensor_text.contains("magnetometer")
            || sensor_text.contains("magnetic field")
            || sensor_text.contains("compass");
        let has_rotation = sensor_text.contains("rotation vector");
        let mut missing = Vec::new();
        if !has_accel {
            missing.push("accelerometer");
        }
        if !has_gyro {
            missing.push("gyroscope");
        }
        if !has_magnet {
            missing.push("magnetometer");
        }
        tells.push(EmulatorTell {
            check: "Motion sensors".into(),
            detected: !missing.is_empty(),
            detail: if missing.is_empty() {
                format!(
                    "accel/gyro/magnetometer co; rotation_vector={}",
                    if has_rotation { "co" } else { "thieu" }
                )
            } else {
                format!("thieu: {}", missing.join(", "))
            },
        });

        let sensor_provider_tells: Vec<&str> = [
            "goldfish",
            "ranchu",
            "qemu",
            "virtual sensor",
            "mock sensor",
        ]
        .into_iter()
        .filter(|needle| sensor_text.contains(needle))
        .collect();
        tells.push(EmulatorTell {
            check: "Sensor provider tells".into(),
            detected: !sensor_provider_tells.is_empty(),
            detail: if sensor_provider_tells.is_empty() {
                "khong thay chuoi sensor gia lap pho bien".into()
            } else {
                sensor_provider_tells.join(", ")
            },
        });

        let bc = self.prop(idx, "ro.build.characteristics").await;
        tells.push(EmulatorTell {
            check: "ro.build.characteristics".into(),
            detected: bc.contains("tablet"),
            detail: bc,
        });

        let tags = self.prop(idx, "ro.build.tags").await;
        tells.push(EmulatorTell {
            check: "ro.build.tags".into(),
            detected: tags == "test-keys",
            detail: if tags.is_empty() {
                "rỗng".into()
            } else {
                tags
            },
        });

        // Có resetprop? → khóa được ro.product.model/android_id (chống MuMu random model).
        // KHÔNG có → model bị ghi đè, fingerprint coherent lệch. Kiểm CẢ binary magisk MPM
        // đẩy vào (/data/local/tmp/magisk) LẪN resetprop standalone (Magisk hệ thống).
        let vm = crate::magisk::VM_MAGISK_PATH;
        let rp = String::from_utf8_lossy(
            &self
                .adb(
                    idx,
                    &format!("shell ([ -x {vm} ] && {vm} -c >/dev/null 2>&1 && echo yes) || command -v resetprop 2>/dev/null || for p in /data/adb/magisk/resetprop /debug_ramdisk/resetprop /sbin/resetprop; do [ -x \"$p\" ] && echo yes && break; done"),
                )
                .await
                .unwrap_or_default(),
        )
        .to_lowercase();
        let has_resetprop = rp.contains("resetprop") || rp.contains("yes");
        tells.push(EmulatorTell {
            check: "Magisk/resetprop (khóa model)".into(),
            // detected=true = CÓ VẤN ĐỀ: thiếu resetprop → model KHÔNG khóa được.
            detected: !has_resetprop,
            detail: if has_resetprop {
                "co resetprop - khoa duoc model/fingerprint/characteristics runtime".into()
            } else {
                "THIẾU — model bị MuMu ghi đè (đặt Magisk APK trong Cài đặt)".into()
            },
        });

        Ok(tells)
    }

    async fn harden(&self, idx: u32) -> AppResult<()> {
        // MuMu 12 không có `su` — root qua adb root; các lệnh dưới chạy trực tiếp.
        let _ = self.adb(idx, "root").await;
        // Xóa prop camera giả (runtime-settable).
        let _ = self.adb(idx, "shell setprop qemu.sf.fake_camera ''").await;
        // Ẩn thư mục Share của MuMu ($MuMu12Shared) và biến thể cũ.
        let _ = self.adb(idx, "shell for p in /mnt/shared \"/sdcard/\\$MuMu12Shared\" \"/storage/emulated/0/\\$MuMu12Shared\" /sdcard/Android/data/com.microvirt.tools/files; do mountpoint -q \"$p\" && umount \"$p\"; done").await;

        // Giả lập pin: rút sạc AC/USB, set mức pin ngẫu nhiên
        let mut rng = crate::humanize::Rng::from_entropy();
        let level = rng.irange(15, 95);
        let _ = self.adb(idx, &format!("shell dumpsys battery set ac 0; dumpsys battery set usb 0; dumpsys battery set level {level}")).await;

        Ok(())
    }

    async fn push_resetprop(&self, idx: u32, local_bin: &str) -> AppResult<bool> {
        let vm = crate::magisk::VM_MAGISK_PATH;
        // MuMu 12 KHÔNG có binary `su` — adbd chạy root qua `adb root`; chạy lệnh TRỰC TIẾP
        // (không bọc `su -c`, vốn fail "su: not found" trên build Android 15).
        let _ = self.adb(idx, "root").await;
        // Best-effort: push hỏng chớp nhoáng (device offline) → Ok(false), KHÔNG Err (nhất quán
        // với chmod/verify tolerant; caller coi đây là no-op chứ không hủy provision).
        if self.adb_args(idx, &["push", local_bin, vm]).await.is_err() {
            tracing::warn!(idx, "Không push được magisk binary vào VM");
            return Ok(false);
        }
        let _ = self.adb(idx, &format!("shell chmod 755 {vm}")).await;
        // Verify bằng ĐÚNG tiêu chí resolver ở lock_device_identity dùng (`magisk -c` exit 0),
        // để hai chỗ không bao giờ bất đồng về "binary chạy được không".
        let v = self
            .adb(
                idx,
                &format!("shell [ -x {vm} ] && {vm} -c >/dev/null 2>&1 && echo MOK"),
            )
            .await
            .unwrap_or_default();
        let ok = String::from_utf8_lossy(&v).contains("MOK");
        if !ok {
            tracing::warn!(idx, "Đẩy magisk binary nhưng chạy thử (magisk -c) thất bại");
        }
        Ok(ok)
    }

    async fn lock_device_identity(&self, idx: u32, hw: &HardwareProfile) -> AppResult<bool> {
        if hw.build_fingerprint.is_empty() {
            return Ok(false); // hồ sơ cũ chưa có fingerprint → không có gì để khóa
        }
        let vm = crate::magisk::VM_MAGISK_PATH;
        // MuMu 12 KHÔNG có `su` — adbd chạy root qua `adb root`; mọi lệnh dưới chạy TRỰC TIẾP.
        let _ = self.adb(idx, "root").await;
        // Lệnh resetprop: (a) binary magisk MPM đã ĐẨY vào ("<vm> resetprop"), hoặc
        // (b) resetprop standalone (nếu cài Magisk hệ thống). Không có → no-op.
        let rp = {
            let m = self
                .adb(
                    idx,
                    &format!("shell [ -x {vm} ] && {vm} -c >/dev/null 2>&1 && echo MOK"),
                )
                .await
                .unwrap_or_default();
            if String::from_utf8_lossy(&m).contains("MOK") {
                Some(format!("{vm} resetprop"))
            } else {
                let probe = self
                    .adb(
                        idx,
                        "shell command -v resetprop 2>/dev/null || for p in /data/adb/magisk/resetprop /debug_ramdisk/resetprop /sbin/resetprop; do [ -x $p ] && echo $p && break; done",
                    )
                    .await
                    .unwrap_or_default();
                String::from_utf8_lossy(&probe)
                    .lines()
                    .map(str::trim)
                    .rfind(|l| !l.is_empty() && !l.contains("already connected"))
                    .map(|s| s.to_string())
                    .filter(|s| s == "resetprop" || s.starts_with('/'))
            }
        };
        let Some(rp) = rp else {
            tracing::warn!(
                idx,
                "VM chưa có resetprop/magisk — bỏ qua khóa model (đặt Magisk APK trong Cài đặt)"
            );
            return Ok(false);
        };
        // KHÔNG chạy từng `su -c 'resetprop KEY "VAL"'`: value CÓ KHOẢNG TRẮNG (vd
        // "Redmi Note 8") bị adb-shell tách lại qua 3 tầng sh → resetprop nhận 3 tham số,
        // model KHÔNG khóa (kiểm chứng thực). Thay vào: SINH script, đẩy vào VM, chạy bằng
        // `sh <file>` — sh đọc nháy kép TỪ FILE nên value giữ nguyên (kiểm chứng: model
        // "Redmi Note 8" khóa đúng). `resetprop -f` bị SELinux chặn đọc file → không dùng.
        let script = build_lock_script(&rp, hw);

        // 1. Xóa các prop đặc trưng của QEMU và MuMu
        // 2. Set các prop cơ bản của phần cứng
        // 3. Set các prop để giả mạo Build Type (tránh userdebug/test-keys)
        // Ẩn ADB (USB debug)
        // 4. Ẩn các file/device node máy ảo (bằng cách dùng bind mount đè /dev/null lên)
        let host = std::env::temp_dir().join(format!("mpm-lock-{idx}.sh"));
        if let Err(e) = fs::write(&host, script.as_bytes()) {
            tracing::warn!(idx, error = %e, "Không ghi được script khóa model tạm");
            return Ok(false);
        }
        let remote = "/data/local/tmp/mpm-lock.sh";
        let Some(host_str) = host.to_str() else {
            let _ = fs::remove_file(&host);
            tracing::warn!(idx, "Duong dan temp script khong phai UTF-8");
            return Ok(false);
        };
        let pushed = self.adb_args(idx, &["push", host_str, remote]).await;
        let _ = fs::remove_file(&host);
        if pushed.is_err() {
            tracing::warn!(idx, "Không đẩy được script khóa model vào VM");
            return Ok(false);
        }
        // Chạy + VERIFY, thử lại 1 lần (adb thi thoảng 'device offline' chớp nhoáng). VERIFY
        // CẢ model LẪN build.fingerprint: chỉ kiểm model có thể báo "khóa" nhầm khi model ăn
        // nhưng fingerprint hỏng → đúng cái INCOHERENCE tính năng này ngăn (finding verify).
        for _ in 0..2 {
            let _ = self.adb(idx, &format!("shell sh {remote}")).await;
            let model_ok = self.prop(idx, "ro.product.model").await == hw.model;
            let fp_ok = self.prop(idx, "ro.build.fingerprint").await == hw.build_fingerprint;
            let characteristics = self.prop(idx, "ro.build.characteristics").await;
            let characteristics_ok = if hw.build_characteristics.is_empty() {
                !characteristics.contains("tablet")
            } else {
                characteristics == hw.build_characteristics
            };
            if model_ok && fp_ok && characteristics_ok {
                let _ = self.adb(idx, &format!("shell rm -f {remote}")).await;
                tracing::info!(idx, model = %hw.model, "Da KHOA model + fingerprint + characteristics qua resetprop (script)");
                return Ok(true);
            }
        }
        let _ = self.adb(idx, &format!("shell rm -f {remote}")).await;
        tracing::warn!(idx, model = %hw.model, "Khoa model/fingerprint/characteristics KHONG verify duoc sau 2 lan");
        Ok(false)
    }

    async fn human_tap(&self, idx: u32, x: i32, y: i32) -> AppResult<()> {
        let (jx, jy, hold) = humanize::human_tap(x, y, &mut Rng::from_entropy());
        // `input swipe` tới CÙNG điểm với thời lượng = giữ ngón → tap có press-duration.
        self.adb(
            idx,
            &format!("shell input swipe {jx} {jy} {jx} {jy} {hold}"),
        )
        .await
        .map(|_| ())
    }

    async fn human_swipe(&self, idx: u32, x0: i32, y0: i32, x1: i32, y1: i32) -> AppResult<()> {
        let mut rng = Rng::from_entropy();
        let path = humanize::human_swipe((x0, y0), (x1, y1), &mut rng);
        // Dùng endpoint ĐÃ RUNG + tổng thời lượng ngẫu nhiên. (Đường cong Bézier đầy đủ
        // trong `path` dành cho executor sendevent tương lai; `input swipe` chỉ nhận 2 đầu.)
        // path luôn có ≥17 điểm (human_swipe) nên first/last an toàn.
        let a = path[0];
        let b = path[path.len() - 1];
        let total: u64 = path.iter().map(|s| s.delay_ms).sum::<u64>().max(50);
        self.adb(
            idx,
            &format!(
                "shell input swipe {} {} {} {} {}",
                a.x, a.y, b.x, b.y, total
            ),
        )
        .await
        .map(|_| ())
    }

    async fn upload_media(&self, idx: u32, local_path: &str) -> AppResult<()> {
        // MuMu 12 không có `su` — root qua adb root; lệnh dưới chạy trực tiếp.
        let _ = self.adb(idx, "root").await;
        let path = std::path::Path::new(local_path);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("video.mp4");
        let remote_name = safe_remote_media_name(name);
        // Tạo thư mục nếu chưa có
        let _ = self.adb(idx, "shell mkdir -p /sdcard/DCIM/Camera").await;

        let remote_path = format!("/sdcard/DCIM/Camera/{remote_name}");
        self.adb_args(idx, &["push", local_path, &remote_path])
            .await?;

        // Gửi broadcast để hệ điều hành quét lại thư viện media
        let uri = shell_quote(&format!("file://{remote_path}"));
        self.adb(
            idx,
            &format!(
                "shell am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d {uri}"
            ),
        )
        .await?;

        tracing::info!(idx, file = %remote_name, "Đã đẩy file media vào VM và quét thư viện");
        Ok(())
    }
}

// ------------------------ Mock (in-memory) ------------------------

/// Mô phỏng thiết bị: giữ "dữ liệu app" theo index trong bộ nhớ. Cho phép test
/// vòng tròn backup→restore mà không cần MuMu/root.
pub struct MockAdbWorker {
    devices: Mutex<HashMap<u32, Vec<u8>>>,
    android_ids: Mutex<HashMap<u32, String>>,
    display_profiles: Mutex<HashMap<u32, (u32, u32, u32)>>,
    locked_models: Mutex<HashMap<u32, String>>,
    lock_counts: Mutex<HashMap<u32, u32>>,
    apk_versions: Mutex<HashMap<u32, String>>,
}

impl MockAdbWorker {
    pub fn new() -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            android_ids: Mutex::new(HashMap::new()),
            display_profiles: Mutex::new(HashMap::new()),
            locked_models: Mutex::new(HashMap::new()),
            lock_counts: Mutex::new(HashMap::new()),
            apk_versions: Mutex::new(HashMap::new()),
        }
    }

    /// Model đã bị khóa qua `lock_device_identity` cho một VM (kiểm wiring).
    #[cfg(test)]
    pub fn locked_model_of(&self, idx: u32) -> Option<String> {
        self.locked_models.lock().unwrap().get(&idx).cloned()
    }

    #[cfg(test)]
    pub fn lock_count_of(&self, idx: u32) -> u32 {
        self.lock_counts
            .lock()
            .unwrap()
            .get(&idx)
            .copied()
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn display_profile_of(&self, idx: u32) -> Option<(u32, u32, u32)> {
        self.display_profiles.lock().unwrap().get(&idx).copied()
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
            apk_version: self.apk_version(idx, _pkg).await?,
        })
    }

    async fn restore(&self, idx: u32, _pkg: &str, archive: &Path) -> AppResult<()> {
        let data = fs::read(archive)?;
        self.devices.lock().unwrap().insert(idx, data);
        Ok(())
    }

    async fn apk_version(&self, idx: u32, _pkg: &str) -> AppResult<String> {
        Ok(self
            .apk_versions
            .lock()
            .unwrap()
            .get(&idx)
            .cloned()
            .unwrap_or_else(|| "mock-1.0".into()))
    }

    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()> {
        self.android_ids
            .lock()
            .unwrap()
            .insert(idx, android_id.to_string());
        Ok(())
    }

    async fn apply_display_profile(
        &self,
        idx: u32,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> AppResult<bool> {
        self.display_profiles
            .lock()
            .unwrap()
            .insert(idx, (width, height, dpi));
        Ok(true)
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

    async fn scan_emulator_tells(&self, _idx: u32) -> AppResult<Vec<EmulatorTell>> {
        // Mẫu khớp thực tế MuMu: chỉ còn native-bridge + hypervisor lộ.
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
            EmulatorTell {
                check: "Motion sensors".into(),
                detected: true,
                detail: "mock: can chay A.16 tren MuMu that de do accel/gyro/magnetometer".into(),
            },
            EmulatorTell {
                check: "Sensor provider tells".into(),
                detected: false,
                detail: "mock".into(),
            },
            EmulatorTell {
                check: "Magisk/resetprop (khóa model)".into(),
                detected: true,
                detail: "THIẾU — model bị MuMu ghi đè (đặt Magisk APK trong Cài đặt)".into(),
            },
        ])
    }

    async fn harden(&self, _idx: u32) -> AppResult<()> {
        Ok(())
    }

    async fn push_resetprop(&self, _idx: u32, _local_bin: &str) -> AppResult<bool> {
        Ok(false)
    }

    async fn lock_device_identity(&self, idx: u32, hw: &HardwareProfile) -> AppResult<bool> {
        if hw.build_fingerprint.is_empty() {
            return Ok(false);
        }
        *self.lock_counts.lock().unwrap().entry(idx).or_insert(0) += 1;
        self.locked_models
            .lock()
            .unwrap()
            .insert(idx, hw.model.clone());
        Ok(true)
    }

    async fn human_tap(&self, _idx: u32, _x: i32, _y: i32) -> AppResult<()> {
        Ok(())
    }

    async fn human_swipe(
        &self,
        _idx: u32,
        _x0: i32,
        _y0: i32,
        _x1: i32,
        _y1: i32,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn upload_media(&self, _idx: u32, _local_path: &str) -> AppResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mpm_adb_{}_{tag}.bin", std::process::id()))
    }

    fn hw_lock() -> HardwareProfile {
        HardwareProfile {
            model: "Redmi Note 8".into(),
            brand: "Redmi".into(),
            manufacturer: "Xiaomi".into(),
            imei: "861000000000000".into(),
            android_id: "a1b2c3d4e5f60718".into(),
            mac: "02:00:00:11:22:33".into(),
            res_width: 1080,
            res_height: 2340,
            dpi: 440,
            device: "ginkgo".into(),
            build_fingerprint:
                "Redmi/ginkgo/ginkgo:11/RP1A.200720.011/V12.5.1.0.RCOMIXM:user/release-keys".into(),
            soc_hardware: "qcom".into(),
            board_platform: "trinket".into(),
            gpu_egl: "adreno".into(),
            security_patch: "2021-05-01".into(),
            build_characteristics: "".into(),
        }
    }

    #[test]
    fn build_lock_script_giu_gia_tri_co_khoang_trang_va_coherent() {
        let s = build_lock_script("magisk resetprop", &hw_lock());
        assert!(
            s.contains("magisk resetprop ro.product.model 'Redmi Note 8'"),
            "{s}"
        );
        assert!(s.contains("magisk resetprop ro.hardware 'qcom'"));
        assert!(s.contains("magisk resetprop ro.board.platform 'trinket'"));
        assert!(s.contains("magisk resetprop ro.hardware.egl 'adreno'"));
        assert!(s.contains("magisk resetprop ro.build.version.security_patch '2021-05-01'"));
        assert!(s.contains("magisk resetprop --delete ro.build.characteristics"));
        assert!(!s.contains("tablet"), "khong duoc de lai tell tablet");

        let hw_empty = HardwareProfile {
            soc_hardware: String::new(),
            ..hw_lock()
        };
        let s2 = build_lock_script("resetprop", &hw_empty);
        assert!(
            !s2.contains("resetprop ro.hardware '"),
            "rong phai bo qua, khong set"
        );
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
