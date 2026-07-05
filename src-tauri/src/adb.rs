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
use tokio::time::{sleep, timeout, Duration};

use crate::error::{AppError, AppResult};
use crate::humanize::{self, Rng};
use crate::model::{EmulatorTell, HardwareProfile, SnapshotMeta};
use crate::snapshot::sha256_file;

/// Trần thời gian cho một lệnh `memuc adb`. Đủ rộng cho thao tác nặng nhất
/// (install APK ~220MB, backup/restore) nhưng vẫn chặn treo vô hạn nếu adb đơ.
const ADB_TIMEOUT: Duration = Duration::from_secs(300);

#[async_trait]
pub trait AdbWorker: Send + Sync {
    /// Backup thư mục data của `pkg` trong VM `idx` ra file `out` (archive).
    async fn backup(&self, idx: u32, pkg: &str, out: &Path) -> AppResult<SnapshotMeta>;
    /// Nạp `archive` vào `/data/data/<pkg>` của VM `idx` (kèm chown + restorecon).
    async fn restore(&self, idx: u32, pkg: &str, archive: &Path) -> AppResult<()>;
    /// Đặt Android ID (qua adb, không phải khoá memuc — §15 thiết kế).
    async fn apply_android_id(&self, idx: u32, android_id: &str) -> AppResult<()>;
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
    /// Ẩn/sửa các dấu vết SỬA ĐƯỢC (best-effort; ro.* cần reboot mới ăn).
    async fn harden(&self, idx: u32) -> AppResult<()>;
    /// KHÓA định danh thiết bị (ro.product.model/brand/manufacturer/device +
    /// ro.build.fingerprint) SAU boot bằng **resetprop** — chống MEmu random model.
    /// Best-effort: trả `Ok(false)` nếu VM chưa có resetprop (cần Magisk trong base
    /// image — xem docs/BASE_IMAGE_MAGISK_SETUP.md); `Ok(true)` nếu khóa & verify được.
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
    ///
    /// - **CREATE_NO_WINDOW**: ẩn cửa sổ console → KHÔNG nhấp nháy khi poll boot
    ///   (mỗi 3s) hay gọi adb liên tục (fix "cửa sổ cmd chớp nháy").
    /// - **kill_on_drop + timeout**: hết giờ thì hủy tiến trình con, không treo vô hạn.
    async fn adb(&self, idx: u32, adb_arg: &str) -> AppResult<Vec<u8>> {
        let mut cmd = Command::new(&self.memuc_path);
        cmd.args(["-i", &idx.to_string(), "adb", adb_arg]);
        cmd.kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let output = timeout(ADB_TIMEOUT, cmd.output())
            .await
            .map_err(|_| AppError::Timeout(ADB_TIMEOUT.as_secs()))??;
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


    async fn wait_boot_completed(&self, idx: u32) -> AppResult<()> {
        use tokio::time::sleep;
        // Poll tối đa ~180s. Boot có thể vượt 90s khi host tải nặng (nhiều VM chạy) —
        // phát hiện qua test thực. `memuc adb` có thể chèn dòng "already connected ..."
        // trước giá trị → lấy token cuối cùng để so sánh (bug đã gặp trên MEmu thật).
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
        // ⚠️ --no-streaming BẮT BUỘC: MEmu KHÔNG hỗ trợ streamed install — adb mặc định
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
            let mut cmd = Command::new(&self.memuc_path);
            cmd.args(["-i", &idx.to_string(), "adb", &arg]);
            cmd.kill_on_drop(true);
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let output = timeout(ADB_TIMEOUT, cmd.output())
                .await
                .map_err(|_| AppError::Timeout(ADB_TIMEOUT.as_secs()))??;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stdout.contains("Success") || stderr.contains("Success") {
                return Ok(());
            }
            last = format!("out={}, err={}", stdout.trim(), stderr.trim());
            tracing::warn!(idx, attempt, max = MAX_TRIES, "adb install lỗi, thử lại: {last}");
            if attempt < MAX_TRIES {
                sleep(Duration::from_secs(4)).await;
            }
        }
        Err(AppError::CommandFailed(format!(
            "adb install thất bại sau {MAX_TRIES} lần: {last}"
        )))
    }

    async fn disable_app(&self, idx: u32, pkg: &str) -> AppResult<()> {
        // Shell user có quyền disable-user (kiểm chứng trên MEmu thật) — không cần root.
        self.adb(idx, &format!("shell pm disable-user --user 0 {pkg}"))
            .await
            .map(|_| ())
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
            // Chỉ tính là "có" khi ls in ra đúng đường dẫn, KHÔNG kèm thông báo lỗi
            // (No such / Permission denied / not found) — tránh dương tính giả.
            let errored = ["No such", "Permission denied", "not found"]
                .iter()
                .any(|e| s.contains(e));
            if s.contains(f) && !errored {
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

    async fn lock_device_identity(&self, idx: u32, hw: &HardwareProfile) -> AppResult<bool> {
        if hw.build_fingerprint.is_empty() {
            return Ok(false); // hồ sơ cũ chưa có fingerprint → không có gì để khóa
        }
        // 1) Tìm resetprop (applet Magisk) ở PATH hoặc các vị trí phổ biến.
        let probe = self
            .adb(
                idx,
                "shell su -c 'command -v resetprop 2>/dev/null || for p in /data/adb/magisk/resetprop /debug_ramdisk/resetprop /sbin/resetprop; do [ -x \"$p\" ] && echo \"$p\" && break; done'",
            )
            .await
            .unwrap_or_default();
        let rp = String::from_utf8_lossy(&probe)
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.contains("already connected"))
            .map(|s| s.to_string());
        let Some(rp) = rp.filter(|s| s == "resetprop" || s.starts_with('/')) else {
            tracing::warn!(
                idx,
                "VM chưa có resetprop — bỏ qua khóa model (cần Magisk trong base image)"
            );
            return Ok(false);
        };
        // 2) Áp trọn bộ props nhất quán (giá trị bọc nháy kép để chịu khoảng trắng như "Redmi Note 8").
        let props: [(&str, &str); 6] = [
            ("ro.product.model", &hw.model),
            ("ro.product.brand", &hw.brand),
            ("ro.product.manufacturer", &hw.manufacturer),
            ("ro.product.device", &hw.device),
            ("ro.product.name", &hw.device),
            ("ro.build.fingerprint", &hw.build_fingerprint),
        ];
        for (key, val) in props {
            if val.is_empty() {
                continue;
            }
            let _ = self
                .adb(idx, &format!("shell su -c '{rp} {key} \"{val}\"'"))
                .await;
        }
        // 3) Verify: model runtime đã bằng giá trị ta khóa chưa.
        Ok(self.prop(idx, "ro.product.model").await == hw.model)
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
}

// ------------------------ Mock (in-memory) ------------------------

/// Mô phỏng thiết bị: giữ "dữ liệu app" theo index trong bộ nhớ. Cho phép test
/// vòng tròn backup→restore mà không cần MEmu/root.
pub struct MockAdbWorker {
    devices: Mutex<HashMap<u32, Vec<u8>>>,
    android_ids: Mutex<HashMap<u32, String>>,
    locked_models: Mutex<HashMap<u32, String>>,
}

impl MockAdbWorker {
    pub fn new() -> Self {
        Self {
            devices: Mutex::new(HashMap::new()),
            android_ids: Mutex::new(HashMap::new()),
            locked_models: Mutex::new(HashMap::new()),
        }
    }

    /// Model đã bị khóa qua `lock_device_identity` cho một VM (kiểm wiring).
    #[cfg(test)]
    pub fn locked_model_of(&self, idx: u32) -> Option<String> {
        self.locked_models.lock().unwrap().get(&idx).cloned()
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

    async fn lock_device_identity(&self, idx: u32, hw: &HardwareProfile) -> AppResult<bool> {
        if hw.build_fingerprint.is_empty() {
            return Ok(false);
        }
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
